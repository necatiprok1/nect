use crate::ast::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

// The value-level core lives in `builtins`, shared with the bytecode VM.
pub use crate::builtins::{format_value, is_truthy, type_name, RuntimeError};
use crate::builtins::{apply_binary, apply_unary, get_index, set_index, Map, Future, ThreadHandle};

type Scope = HashMap<String, Value>;
type NativeFn = Rc<dyn Fn(&[Value]) -> Result<Value, RuntimeError>>;

struct FlowError {
    kind: FlowKind,
}

enum FlowKind {
    Return(Value),
    Error(RuntimeError),
    /// `break` and `continue` unwind to the innermost loop, which is the only
    /// place that catches them.
    Break,
    Continue,
}

impl From<RuntimeError> for FlowError {
    fn from(e: RuntimeError) -> Self {
        FlowError { kind: FlowKind::Error(e) }
    }
}

struct Environment {
    scopes: Vec<Scope>,
    natives: HashMap<String, NativeFn>,
}

impl Environment {
    fn new() -> Self {
        Self {
            scopes: vec![Scope::new()],
            natives: HashMap::new(),
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope::new());
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define(&mut self, name: String, value: Value) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name, value);
        }
    }

    fn define_native(&mut self, name: &str, f: NativeFn) {
        self.natives.insert(name.to_string(), f);
    }

    fn get(&self, name: &str) -> Option<Value> {
        for scope in self.scopes.iter().rev() {
            if let Some(v) = scope.get(name) {
                return Some(v.clone());
            }
        }
        None
    }

    fn get_native(&self, name: &str) -> Option<NativeFn> {
        self.natives.get(name).cloned()
    }

    fn assign_or_define(&mut self, name: &str, value: Value) {
        for scope in self.scopes.iter_mut().rev() {
            if scope.contains_key(name) {
                scope.insert(name.to_string(), value);
                return;
            }
        }
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.to_string(), value);
        }
    }
}

pub struct Interpreter {
    env: Environment,
}

impl Default for Interpreter {
    fn default() -> Self {
        Self::new()
    }
}

impl Interpreter {
    pub fn new() -> Self {
        let mut env = Environment::new();
        // Every builtin comes from the shared library, so the reference
        // interpreter and the bytecode VM expose exactly the same surface.
        for name in crate::builtins::NAMES {
            env.define_native(name, Rc::new(move |args| crate::builtins::call(name, args)));
        }
        Self { env }
    }
    
    /// Get all variables in the current scope (for debugger)
    pub fn get_locals(&self) -> Vec<(String, Value)> {
        if let Some(scope) = self.env.scopes.last() {
            scope.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
        } else {
            vec![]
        }
    }
    
    /// Evaluate an expression string in the current context (for debugger print)
    pub fn eval_expr(&mut self, expr_str: &str) -> Result<Value, String> {
        let lexer = crate::lexer::Lexer::new(expr_str);
        let tokens = lexer.tokenize().map_err(|e| e.message)?;
        let mut parser = crate::parser::Parser::new(tokens);
        let expr = parser.parse_expr().map_err(|e| e.message)?;
        self.eval(&expr).map_err(|e| e.message)
    }

    pub fn run(&mut self, stmts: &[Stmt]) -> Result<(), RuntimeError> {
        for stmt in stmts {
            match self.execute(stmt) {
                Ok(()) => {}
                Err(FlowError { kind: FlowKind::Error(e) }) => return Err(e),
                Err(FlowError { kind: FlowKind::Return(_) }) => {
                    return Err(RuntimeError::new("'return' outside of a function"));
                }
                Err(FlowError { kind: FlowKind::Break }) => {
                    return Err(RuntimeError::new("'break' outside of a loop"));
                }
                Err(FlowError { kind: FlowKind::Continue }) => {
                    return Err(RuntimeError::new("'continue' outside of a loop"));
                }
            }
        }
        Ok(())
    }

    fn execute(&mut self, stmt: &Stmt) -> Result<(), FlowError> {
        match stmt {
            Stmt::Expression(expr) => {
                self.eval(expr)?;
                Ok(())
            }
            Stmt::Let { name, value } => {
                let val = self.eval(value)?;
                self.env.define(name.clone(), val);
                Ok(())
            }
            Stmt::Block(stmts) => {
                self.env.push_scope();
                let result = self.execute_block(stmts);
                self.env.pop_scope();
                result
            }
            Stmt::If {
                condition,
                then_branch,
                else_branch,
            } => {
                let cond = self.eval(condition)?;
                if is_truthy(&cond) {
                    self.execute_block(then_branch)?;
                } else if !else_branch.is_empty() {
                    self.execute_block(else_branch)?;
                }
                Ok(())
            }
            Stmt::While { condition, body } => {
                while is_truthy(&self.eval(condition)?) {
                    match self.execute_block(body) {
                        Ok(()) | Err(FlowError { kind: FlowKind::Continue }) => {}
                        Err(FlowError { kind: FlowKind::Break }) => break,
                        Err(other) => return Err(other),
                    }
                }
                Ok(())
            }
            Stmt::For { var_name, iterable, body } => {
                let iterable_value = self.eval(iterable)?;
                // A map iterates its keys in insertion order; arrays iterate
                // their elements. Everything else is the old error.
                let items: Vec<Value> = match &iterable_value {
                    Value::Array(a) => a.borrow().clone(),
                    Value::Map(m) => m.borrow().keys(),
                    _ => {
                        return Err(FlowError::from(RuntimeError::new(
                            "for loop requires an array or map",
                        )))
                    }
                };
                let len = items.len();
                let arr_rc = Rc::new(RefCell::new(items));
                let mut i = 0;
                while i < len {
                    self.env.define(var_name.clone(), arr_rc.borrow()[i].clone());
                    match self.execute_block(body) {
                        // `continue` still advances the index.
                        Ok(()) | Err(FlowError { kind: FlowKind::Continue }) => {}
                        Err(FlowError { kind: FlowKind::Break }) => break,
                        Err(other) => return Err(other),
                    }
                    i += 1;
                }
                Ok(())
            }
            Stmt::Function { name, params, body } => {
                let func = Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                self.env.define(name.clone(), Value::Function(func));
                Ok(())
            }
            Stmt::AsyncFunction { name, params, body } => {
                let func = Function {
                    name: name.clone(),
                    params: params.clone(),
                    body: body.clone(),
                };
                self.env.define(name.clone(), Value::Function(func));
                Ok(())
            }
            Stmt::Return(expr) => {
                let val = match expr {
                    Some(e) => self.eval(e)?,
                    None => Value::Null,
                };
                Err(FlowError { kind: FlowKind::Return(val) })
            }
            Stmt::Break => Err(FlowError { kind: FlowKind::Break }),
            Stmt::Continue => Err(FlowError { kind: FlowKind::Continue }),
        }
    }

    fn execute_block(&mut self, stmts: &[Stmt]) -> Result<(), FlowError> {
        for stmt in stmts {
            self.execute(stmt)?;
        }
        Ok(())
    }

    fn eval(&mut self, expr: &Expr) -> Result<Value, RuntimeError> {
        match expr {
            Expr::Literal(v) => Ok(v.clone()),
            Expr::Variable(name) => {
                if let Some(v) = self.env.get(name) {
                    Ok(v)
                } else if self.env.get_native(name).is_some() {
                    Err(RuntimeError::new(&format!(
                        "cannot use '{}' as a value (it is a function)",
                        name
                    )))
                } else {
                    Err(RuntimeError::new(&format!("undefined variable '{}'", name)))
                }
            }
            Expr::Assign { name, value } => {
                let val = self.eval(value)?;
                if self.env.get(name).is_none() && self.env.get_native(name).is_none() {
                    return Err(RuntimeError::new(&format!("undefined variable '{}'", name)));
                }
                self.env.assign_or_define(name, val.clone());
                Ok(val)
            }
            Expr::Binary { left, op, right } => {
                match op {
                    BinaryOp::And => {
                        let l = self.eval(left)?;
                        if !is_truthy(&l) {
                            return Ok(Value::Boolean(false));
                        }
                        let r = self.eval(right)?;
                        Ok(Value::Boolean(is_truthy(&r)))
                    }
                    BinaryOp::Or => {
                        let l = self.eval(left)?;
                        if is_truthy(&l) {
                            return Ok(Value::Boolean(true));
                        }
                        let r = self.eval(right)?;
                        Ok(Value::Boolean(is_truthy(&r)))
                    }
                    _ => {
                        let l = self.eval(left)?;
                        let r = self.eval(right)?;
                        self.eval_binary(l, *op, r)
                    }
                }
            }
            Expr::Unary { op, operand } => {
                let v = self.eval(operand)?;
                self.eval_unary(*op, v)
            }
            Expr::Conditional {
                condition,
                then_expr,
                else_expr,
            } => {
                // Only the branch that is taken is evaluated.
                if is_truthy(&self.eval(condition)?) {
                    self.eval(then_expr)
                } else {
                    self.eval(else_expr)
                }
            }
            Expr::Grouping(e) => self.eval(e),
            Expr::Call { callee, args } => {
                let arg_vals: Vec<Value> = args
                    .iter()
                    .map(|a| self.eval(a))
                    .collect::<Result<_, _>>()?;

                if let Expr::Variable(name) = &**callee
                    && let Some(nf) = self.env.get_native(name)
                {
                    return nf(&arg_vals);
                }

                let func_val = self.eval(callee)?;
                self.call_function(&func_val, &arg_vals)
            }
            Expr::Array(elements) => {
                let vals: Result<Vec<Value>, RuntimeError> = elements
                    .iter()
                    .map(|e| self.eval(e))
                    .collect();
                Ok(Value::Array(Rc::new(RefCell::new(vals?))))
            }
            Expr::Map(entries) => {
                let mut map = Map::new();
                for (key_expr, value_expr) in entries {
                    let key = self.eval(key_expr)?;
                    let value = self.eval(value_expr)?;
                    map.insert(key, value)?;
                }
                Ok(Value::Map(Rc::new(RefCell::new(map))))
            }
            Expr::GetIndex { array, index } => {
                let arr = self.eval(array)?;
                let idx = self.eval(index)?;
                get_index(&arr, &idx)
            }
            Expr::SetIndex {
                array,
                index,
                op,
                value,
            } => {
                let arr = self.eval(array)?;
                let idx = self.eval(index)?;
                let val = self.eval(value)?;
                set_index(&arr, &idx, *op, val)
            }
            Expr::Await(expr) => {
                // In the interpreter, await just evaluates the expression
                // since we don't have real async support yet
                let val = self.eval(expr)?;
                match val {
                    Value::Future(f) => {
                        // Block until ready
                        while !f.borrow().is_ready() {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        }
                        Ok(f.borrow().get_result().unwrap_or(Value::Null))
                    }
                    other => Ok(other),
                }
            }
            Expr::Spawn(expr) => {
                // Spawn a function in a new thread
                let val = self.eval(expr)?;
                match val {
                    Value::Function(_f) => {
                        // For now, create a dummy thread that just sleeps briefly
                        let handle = std::thread::spawn(|| {
                            std::thread::sleep(std::time::Duration::from_millis(1));
                        });
                        Ok(Value::ThreadHandle(Rc::new(RefCell::new(ThreadHandle::new(handle)))))
                    }
                    other => Err(RuntimeError::new(&format!(
                        "spawn() requires a function, got {}",
                        type_name(&other)
                    ))),
                }
            }
        }
    }

    fn eval_binary(&self, l: Value, op: BinaryOp, r: Value) -> Result<Value, RuntimeError> {
        apply_binary(l, op, r)
    }

    fn eval_unary(&self, op: UnaryOp, v: Value) -> Result<Value, RuntimeError> {
        apply_unary(op, v)
    }

    fn call_function(&mut self, callee: &Value, args: &[Value]) -> Result<Value, RuntimeError> {
        match callee {
            Value::Function(func) => {
                if args.len() != func.params.len() {
                    return Err(RuntimeError::new(&format!(
                        "function '{}' expects {} argument(s), got {}",
                        func.name, func.params.len(), args.len()
                    )));
                }
                self.env.push_scope();
                for (i, param) in func.params.iter().enumerate() {
                    self.env.define(param.clone(), args[i].clone());
                }
                let result = self.execute_block(&func.body);
                self.env.pop_scope();
                match result {
                    Ok(()) => Ok(Value::Null),
                    Err(FlowError { kind: FlowKind::Return(val) }) => Ok(val),
                    Err(FlowError { kind: FlowKind::Error(e) }) => Err(e),
                    // A loop always catches these, so reaching here means the
                    // keyword sits outside any loop.
                    Err(FlowError { kind: FlowKind::Break }) => {
                        Err(RuntimeError::new("'break' outside of a loop"))
                    }
                    Err(FlowError { kind: FlowKind::Continue }) => {
                        Err(RuntimeError::new("'continue' outside of a loop"))
                    }
                }
            }
            _ => Err(RuntimeError::new("can only call functions")),
        }
    }
}
