use crate::ast::{Expr, Stmt};
use crate::lexer::Lexer;
use crate::parser::Parser;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone)]
pub struct Lint {
    pub severity: Severity,
    pub message: String,
    pub line: usize,
    pub column: usize,
    pub rule: &'static str,
}

impl Lint {
    pub fn error(message: String, line: usize, column: usize, rule: &'static str) -> Self {
        Self { severity: Severity::Error, message, line, column, rule }
    }
    pub fn warning(message: String, line: usize, column: usize, rule: &'static str) -> Self {
        Self { severity: Severity::Warning, message, line, column, rule }
    }
    pub fn info(message: String, line: usize, column: usize, rule: &'static str) -> Self {
        Self { severity: Severity::Info, message, line, column, rule }
    }
}

#[derive(Debug, Default)]
pub struct LinterConfig {
    pub unused_variables: bool,
    pub shadowing: bool,
    pub dead_code: bool,
    pub unused_imports: bool,
    pub style: bool,
    pub unused_parameters: bool,
}

impl LinterConfig {
    pub fn all() -> Self {
        Self {
            unused_variables: true,
            shadowing: true,
            dead_code: true,
            unused_imports: true,
            style: true,
            unused_parameters: true,
        }
    }
}

pub struct Linter {
    config: LinterConfig,
    lints: Vec<Lint>,
    scopes: Vec<Scope>,
    used_vars: HashSet<String>,
    defined_vars: HashMap<String, (usize, usize)>, // name -> (line, col)
    loop_depth: usize,
    function_depth: usize,
    in_function_body: bool,
    params_in_current_fn: Vec<String>,
    dead_code_reachable: bool,
}

#[derive(Debug)]
struct Scope {
    vars: HashMap<String, (usize, usize)>, // name -> (line, col)
}

impl Linter {
    pub fn new(config: LinterConfig) -> Self {
        let mut linter = Self {
            config,
            lints: Vec::new(),
            scopes: Vec::new(),
            used_vars: HashSet::new(),
            defined_vars: HashMap::new(),
            loop_depth: 0,
            function_depth: 0,
            in_function_body: false,
            params_in_current_fn: Vec::new(),
            dead_code_reachable: true,
        };
        linter.scopes.push(Scope { vars: HashMap::new() });
        linter
    }

    pub fn lint(&mut self, stmts: &[Stmt]) -> Vec<Lint> {
        self.lints.clear();
        self.used_vars.clear();
        self.defined_vars.clear();
        self.scopes.clear();
        self.scopes.push(Scope { vars: HashMap::new() });
        self.loop_depth = 0;
        self.function_depth = 0;
        self.in_function_body = false;
        self.params_in_current_fn.clear();
        self.dead_code_reachable = true;

        for stmt in stmts {
            self.lint_stmt(stmt);
        }

        // Check for unused variables
        if self.config.unused_variables {
            for scope in &self.scopes {
                for (name, (line, col)) in &scope.vars {
                    if !self.used_vars.contains(name) && !name.starts_with('_') {
                        self.lints.push(Lint::warning(
                            format!("unused variable: `{}`", name),
                            *line,
                            *col,
                            "unused_variables",
                        ));
                    }
                }
            }
        }

        self.lints.clone()
    }

    fn lint_stmt(&mut self, stmt: &Stmt) {
        use Stmt::*;
        match stmt {
            Let { name, value, .. } => {
                self.check_shadowing(name, "variable");
                self.define_var(name.clone(), self.current_pos(value));
                self.lint_expr(value);
            }
            Expression(expr) => self.lint_expr(expr),
            Block(stmts) => {
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                for s in stmts {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        // Only warn once per block
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
            }
            If { condition, then_branch, else_branch } => {
                self.lint_expr(condition);
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                for s in then_branch {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
                if !else_branch.is_empty() {
                    self.push_scope();
                    let old_reachable = self.dead_code_reachable;
                    for s in else_branch {
                        if !self.dead_code_reachable && self.config.dead_code {
                            self.lints.push(Lint::warning(
                                "unreachable code".to_string(),
                                0, 0, "dead_code",
                            ));
                            self.dead_code_reachable = true;
                        }
                        self.lint_stmt(s);
                    }
                    self.pop_scope();
                    self.dead_code_reachable = old_reachable;
                }
            }
            While { condition, body } => {
                self.lint_expr(condition);
                self.loop_depth += 1;
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                for s in body {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
                self.loop_depth -= 1;
            }
            Function { name, params, body } => {
                self.check_shadowing(name, "function");
                self.define_var(name.clone(), (0, 0)); // function name at top level
                self.function_depth += 1;
                
                // Track parameters for unused parameter check
                let old_params = std::mem::take(&mut self.params_in_current_fn);
                self.params_in_current_fn = params.clone();
                
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                self.dead_code_reachable = true;
                for param in params {
                    self.define_var(param.clone(), (0, 0));
                }
                self.in_function_body = true;
                for s in body {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.in_function_body = false;
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
                self.function_depth -= 1;
                
                // Check for unused parameters (use current function's params, not old_params)
                if self.config.unused_parameters {
                    for param in &self.params_in_current_fn {
                        if !self.used_vars.contains(param) && !param.starts_with('_') {
                            self.lints.push(Lint::warning(
                                format!("unused parameter: `{}`", param),
                                0, 0, "unused_parameters",
                            ));
                        }
                    }
                }
                
                self.params_in_current_fn = old_params;
            }
            AsyncFunction { name, params, body } => {
                self.check_shadowing(name, "async function");
                self.define_var(name.clone(), (0, 0));
                self.function_depth += 1;
                
                let old_params = std::mem::take(&mut self.params_in_current_fn);
                self.params_in_current_fn = params.clone();
                
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                self.dead_code_reachable = true;
                for param in params {
                    self.define_var(param.clone(), (0, 0));
                }
                self.in_function_body = true;
                for s in body {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.in_function_body = false;
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
                self.function_depth -= 1;
                
                if self.config.unused_parameters {
                    for param in &self.params_in_current_fn {
                        if !self.used_vars.contains(param) && !param.starts_with('_') {
                            self.lints.push(Lint::warning(
                                format!("unused parameter: `{}`", param),
                                0, 0, "unused_parameters",
                            ));
                        }
                    }
                }
                
                self.params_in_current_fn = old_params;
            }
            Return(expr) => {
                if self.function_depth == 0 {
                    self.lints.push(Lint::error(
                        "`return` outside of function".to_string(),
                        0, 0, "invalid_return",
                    ));
                }
                if let Some(e) = expr {
                    self.lint_expr(e);
                }
                if self.config.dead_code {
                    self.dead_code_reachable = false;
                }
            }
            For { var_name, iterable, body } => {
                self.lint_expr(iterable);
                self.loop_depth += 1;
                self.push_scope();
                let old_reachable = self.dead_code_reachable;
                self.define_var(var_name.clone(), (0, 0));
                for s in body {
                    if !self.dead_code_reachable && self.config.dead_code {
                        self.lints.push(Lint::warning(
                            "unreachable code".to_string(),
                            0, 0, "dead_code",
                        ));
                        self.dead_code_reachable = true;
                    }
                    self.lint_stmt(s);
                }
                self.pop_scope();
                self.dead_code_reachable = old_reachable;
                self.loop_depth -= 1;
            }
            Break => {
                if self.loop_depth == 0 {
                    self.lints.push(Lint::error(
                        "`break` outside of loop".to_string(),
                        0, 0, "invalid_break",
                    ));
                }
                if self.config.dead_code {
                    self.dead_code_reachable = false;
                }
            }
            Continue => {
                if self.loop_depth == 0 {
                    self.lints.push(Lint::error(
                        "`continue` outside of loop".to_string(),
                        0, 0, "invalid_continue",
                    ));
                }
                if self.config.dead_code {
                    self.dead_code_reachable = false;
                }
            }
        }
    }

    fn lint_expr(&mut self, expr: &Expr) {
        use Expr::*;
        match expr {
            Variable(name) => {
                self.used_vars.insert(name.clone());
            }
            Binary { left, right, .. } => {
                self.lint_expr(left);
                self.lint_expr(right);
            }
            Unary { operand, .. } => {
                self.lint_expr(operand);
            }
            Call { callee, args } => {
                self.lint_expr(callee);
                for arg in args {
                    self.lint_expr(arg);
                }
            }
            Array(elements) => {
                for elem in elements {
                    self.lint_expr(elem);
                }
            }
            Map(entries) => {
                for (key, value) in entries {
                    self.lint_expr(key);
                    self.lint_expr(value);
                }
            }
            GetIndex { array, index } => {
                self.lint_expr(array);
                self.lint_expr(index);
            }
            SetIndex { array, index, value, .. } => {
                self.lint_expr(array);
                self.lint_expr(index);
                self.lint_expr(value);
            }
            Conditional { condition, then_expr, else_expr } => {
                self.lint_expr(condition);
                self.lint_expr(then_expr);
                self.lint_expr(else_expr);
            }
            Await(expr) => {
                self.lint_expr(expr);
            }
            Spawn(expr) => {
                self.lint_expr(expr);
            }
            Grouping(expr) => {
                self.lint_expr(expr);
            }
            Assign { name, value } => {
                self.used_vars.insert(name.clone());
                self.lint_expr(value);
            }
            Literal(_) => {}
        }
    }

    fn push_scope(&mut self) {
        self.scopes.push(Scope { vars: HashMap::new() });
    }

    fn pop_scope(&mut self) {
        self.scopes.pop();
    }

    fn define_var(&mut self, name: String, pos: (usize, usize)) {
        if let Some(scope) = self.scopes.last_mut() {
            scope.vars.insert(name.clone(), pos);
        }
    }

    fn check_shadowing(&mut self, name: &str, kind: &str) {
        if !self.config.shadowing {
            return;
        }
        for scope in self.scopes.iter().rev() {
            if scope.vars.contains_key(name) {
                self.lints.push(Lint::warning(
                    format!("`{}` shadows {} from outer scope", name, kind),
                    0, 0, "shadowing",
                ));
                break;
            }
        }
    }

    fn current_pos(&self, _expr: &Expr) -> (usize, usize) {
        (0, 0) // Would need span info from parser
    }
}

pub fn lint_source(source: &str, config: LinterConfig) -> Result<Vec<Lint>, String> {
    let lexer = Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| e.message)?;
    let parser = Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| e.message)?;
    
    let mut linter = Linter::new(config);
    Ok(linter.lint(&stmts))
}

pub fn lint_file(path: &std::path::Path, config: LinterConfig) -> Result<Vec<Lint>, String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    lint_source(&source, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unused_variable() {
        let source = "let x = 42\nlet y = 10";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(lints.iter().any(|l| l.rule == "unused_variables" && l.message.contains("x")));
        assert!(lints.iter().any(|l| l.rule == "unused_variables" && l.message.contains("y")));
    }

    #[test]
    fn test_used_variable() {
        let source = "let x = 42\nprint(x)";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(!lints.iter().any(|l| l.rule == "unused_variables" && l.message.contains("x")));
    }

    #[test]
    fn test_shadowing() {
        let source = "let x = 1\n{ let x = 2 }";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(lints.iter().any(|l| l.rule == "shadowing"));
    }

    #[test]
    fn test_break_outside_loop() {
        let source = "break";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(lints.iter().any(|l| l.rule == "invalid_break"));
    }

    #[test]
    fn test_continue_outside_loop() {
        let source = "continue";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(lints.iter().any(|l| l.rule == "invalid_continue"));
    }

    #[test]
    fn test_return_outside_function() {
        let source = "return 42";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(lints.iter().any(|l| l.rule == "invalid_return"));
    }

    #[test]
    fn test_underscore_prefix_ignored() {
        let source = "let _unused = 42";
        let lints = lint_source(source, LinterConfig::all()).unwrap();
        assert!(!lints.iter().any(|l| l.rule == "unused_variables" && l.message.contains("_unused")));
    }
}