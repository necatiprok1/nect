use std::cell::RefCell;
use std::rc::Rc;

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

#[derive(Debug, Clone, PartialEq)]
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
}
