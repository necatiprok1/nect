use std::cell::RefCell;
use std::rc::Rc;

/// Source code span: byte range [start, end) and line/column for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Span {
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

impl Span {
    pub fn new(start: usize, end: usize, line: usize, col: usize) -> Self {
        Self {
            start,
            end,
            line,
            col,
        }
    }

    pub fn merge(self, other: Span) -> Self {
        if self.start == 0 && self.end == 0 {
            return other;
        }
        if other.start == 0 && other.end == 0 {
            return self;
        }
        Self {
            start: self.start.min(other.start),
            end: self.end.max(other.end),
            line: self.line.min(other.line),
            col: self.col.min(other.col),
        }
    }
}

impl From<crate::lexer::Token> for Span {
    fn from(token: crate::lexer::Token) -> Self {
        Self::new(token.start, token.end, token.line, token.col)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Expr {
    Literal(Value),
    Variable(String),
    Assign {
        name: String,
        value: Box<Expr>,
    },
    Binary {
        left: Box<Expr>,
        op: BinaryOp,
        right: Box<Expr>,
    },
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// Array literal: `[1, 2, 3]`
    Array(Vec<Expr>),
    /// Map literal: `{"a": 1, b: 2}`. Identifier keys are shorthand for the
    /// string of the same name; explicit keys may be any string, number, or
    /// boolean expression. Duplicate keys keep the last value.
    Map(Vec<(Expr, Expr)>),
    /// Array or string element access, `target[index]`.
    ///
    /// Indexing a string yields a one-character string. Negative indices count
    /// back from the end, so `arr[-1]` is the last element.
    GetIndex {
        array: Box<Expr>,
        index: Box<Expr>,
    },
    /// Element assignment: `arr[index] = value`, or `arr[index] += value` when
    /// `op` is set.
    ///
    /// The `op` field exists so a compound assignment evaluates the target and
    /// the index exactly once; simple assignment leaves it `None`.
    SetIndex {
        array: Box<Expr>,
        index: Box<Expr>,
        op: Option<BinaryOp>,
        value: Box<Expr>,
    },
    /// Conditional expression, `condition ? then : else`. Only the taken
    /// branch is evaluated.
    Conditional {
        condition: Box<Expr>,
        then_expr: Box<Expr>,
        else_expr: Box<Expr>,
    },
    /// Await an async computation: `await expr`.
    Await(Box<Expr>),
    /// Spawn a function in a new thread: `spawn(fn)`.
    Spawn(Box<Expr>),
    Grouping(Box<Expr>),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BinaryOp {
    Add,
    Subtract,
    Multiply,
    Divide,
    Modulo,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UnaryOp {
    Negate,
    Not,
}

#[derive(Debug, Clone)]
pub enum Value {
    Number(f64),
    String(String),
    Boolean(bool),
    Null,
    Function(Function),
    Array(Rc<RefCell<Vec<Value>>>),
    /// A map: insertion-ordered key/value pairs. Keys are strings, numbers, or
    /// booleans (the hashable subset); `null`, arrays, maps, and functions are
    /// rejected as keys. `d["key"]` reads, `d["key"] = v` inserts or overwrites.
    Map(Rc<RefCell<crate::builtins::Map>>),
    /// A future/promise from an async computation.
    Future(Rc<RefCell<crate::builtins::Future>>),
    /// A channel for thread communication.
    Channel(Rc<RefCell<crate::builtins::Channel>>),
    /// A mutex for synchronization.
    Mutex(Rc<RefCell<crate::builtins::Mutex>>),
    /// A thread handle returned by spawn.
    ThreadHandle(Rc<RefCell<crate::builtins::ThreadHandle>>),
    /// A SQLite database connection.
    DbConnection(Rc<RefCell<crate::builtins::DbConnection>>),
    /// A GUI window handle for native GUI framework.
    GuiWindow(Rc<RefCell<crate::builtins::GuiWindow>>),
}

impl PartialEq for Value {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Value::Number(a), Value::Number(b)) => a == b,
            (Value::String(a), Value::String(b)) => a == b,
            (Value::Boolean(a), Value::Boolean(b)) => a == b,
            (Value::Null, Value::Null) => true,
            (Value::Function(a), Value::Function(b)) => a == b,
            (Value::Array(a), Value::Array(b)) => *a.borrow() == *b.borrow(),
            (Value::Map(a), Value::Map(b)) => *a.borrow() == *b.borrow(),
            (Value::Future(a), Value::Future(b)) => Rc::ptr_eq(a, b),
            (Value::Channel(a), Value::Channel(b)) => Rc::ptr_eq(a, b),
            (Value::Mutex(a), Value::Mutex(b)) => Rc::ptr_eq(a, b),
            (Value::ThreadHandle(a), Value::ThreadHandle(b)) => Rc::ptr_eq(a, b),
            (Value::DbConnection(a), Value::DbConnection(b)) => Rc::ptr_eq(a, b),
            (Value::GuiWindow(a), Value::GuiWindow(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Function {
    pub name: String,
    pub params: Vec<String>,
    pub body: Vec<Stmt>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Stmt {
    Expression(Expr),
    Let {
        name: String,
        value: Expr,
    },
    Block(Vec<Stmt>),
    If {
        condition: Expr,
        then_branch: Vec<Stmt>,
        else_branch: Vec<Stmt>,
    },
    While {
        condition: Expr,
        body: Vec<Stmt>,
    },
    Function {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    Return(Option<Expr>),
    For {
        var_name: String,
        iterable: Expr,
        body: Vec<Stmt>,
    },
    /// Leaves the innermost enclosing loop.
    Break,
    /// Skips to the next iteration of the innermost enclosing loop.
    Continue,
    /// Async function definition.
    AsyncFunction {
        name: String,
        params: Vec<String>,
        body: Vec<Stmt>,
    },
    /// FFI extern declaration: declares one or more functions from a shared
    /// library. Each entry is `(name, param_types, return_type)` where types
    /// are one of "number", "string", "bool", "void".
    /// The library name is resolved by the platform's dynamic linker
    /// (e.g. "m" -> libm.so on Linux, libm.dylib on macOS).
    Extern {
        library: String,
        functions: Vec<(String, Vec<ExternType>, ExternType)>,
        span: Span,
    },
}

/// Parameter or return type for an extern function declaration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExternType {
    Number,
    String,
    Bool,
    Void,
}
