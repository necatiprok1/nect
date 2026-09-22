use crate::ast::{BinaryOp, Expr, Stmt, UnaryOp, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatConfig {
    pub indent_size: usize,
    pub max_line_width: usize,
    pub brace_style: BraceStyle,
    pub trailing_comma: bool,
    pub single_quote: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BraceStyle {
    SameLine,
    NextLine,
}

impl Default for FormatConfig {
    fn default() -> Self {
        Self {
            indent_size: 4,
            max_line_width: 100,
            brace_style: BraceStyle::SameLine,
            trailing_comma: false,
            single_quote: false,
        }
    }
}

pub struct Formatter {
    config: FormatConfig,
    indent_level: usize,
    output: String,
}

impl Formatter {
    pub fn new(config: FormatConfig) -> Self {
        Self {
            config,
            indent_level: 0,
            output: String::new(),
        }
    }

    pub fn format(&mut self, stmts: &[Stmt]) -> String {
        self.output.clear();
        self.indent_level = 0;
        for (i, stmt) in stmts.iter().enumerate() {
            self.format_stmt(stmt);
            if i < stmts.len() - 1 {
                self.newline();
            }
        }
        self.output.clone()
    }

    fn format_stmt(&mut self, stmt: &Stmt) {
        use Stmt::*;
        match stmt {
            Expression(expr) => {
                self.write_indent();
                self.format_expr(expr);
                self.output.push('\n');
            }
            Let { name, value } => {
                self.write_indent();
                self.output.push_str("let ");
                self.output.push_str(name);
                self.output.push_str(" = ");
                self.format_expr(value);
                self.output.push('\n');
            }
            Block(stmts) => {
                self.write_indent();
                self.output.push_str("{\n");
                self.indent_level += 1;
                for (i, s) in stmts.iter().enumerate() {
                    self.format_stmt(s);
                    if i < stmts.len() - 1 {
                        self.newline();
                    }
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            If { condition, then_branch, else_branch } => {
                self.write_indent();
                self.output.push_str("if ");
                self.format_expr(condition);
                self.output.push(' ');
                self.format_brace_block(then_branch);
                if !else_branch.is_empty() {
                    self.output.push_str(" else ");
                    self.format_brace_block(&else_branch);
                }
                self.output.push('\n');
            }
            While { condition, body } => {
                self.write_indent();
                self.output.push_str("while ");
                self.format_expr(condition);
                self.output.push(' ');
                self.format_brace_block(body);
                self.output.push('\n');
            }
            Function { name, params, body } => {
                self.write_indent();
                self.output.push_str("fn ");
                self.output.push_str(name);
                self.output.push('(');
                self.output.push_str(&params.join(", "));
                self.output.push_str(") ");
                self.format_brace_block(body);
                self.output.push('\n');
            }
            AsyncFunction { name, params, body } => {
                self.write_indent();
                self.output.push_str("async fn ");
                self.output.push_str(name);
                self.output.push('(');
                self.output.push_str(&params.join(", "));
                self.output.push_str(") ");
                self.format_brace_block(body);
                self.output.push('\n');
            }
            Return(Some(expr)) => {
                self.write_indent();
                self.output.push_str("return ");
                self.format_expr(expr);
                self.output.push('\n');
            }
            Return(None) => {
                self.write_indent();
                self.output.push_str("return\n");
            }
            For { var_name, iterable, body } => {
                self.write_indent();
                self.output.push_str("for ");
                self.output.push_str(var_name);
                self.output.push_str(" in ");
                self.format_expr(iterable);
                self.output.push(' ');
                self.format_brace_block(body);
                self.output.push('\n');
            }
            Break => {
                self.write_indent();
                self.output.push_str("break\n");
            }
            Continue => {
                self.write_indent();
                self.output.push_str("continue\n");
            }
        }
    }

    fn format_brace_block(&mut self, stmts: &[Stmt]) {
        match self.config.brace_style {
            BraceStyle::SameLine => {
                self.output.push_str("{\n");
                self.indent_level += 1;
                for (i, s) in stmts.iter().enumerate() {
                    self.format_stmt(s);
                    if i < stmts.len() - 1 {
                        self.newline();
                    }
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
            BraceStyle::NextLine => {
                self.output.push_str("\n");
                self.write_indent();
                self.output.push_str("{\n");
                self.indent_level += 1;
                for (i, s) in stmts.iter().enumerate() {
                    self.format_stmt(s);
                    if i < stmts.len() - 1 {
                        self.newline();
                    }
                }
                self.indent_level -= 1;
                self.write_indent();
                self.output.push('}');
            }
        }
    }

    fn format_expr(&mut self, expr: &Expr) {
        use Expr::*;
        match expr {
            Literal(value) => self.format_value(value),
            Variable(name) => self.output.push_str(name),
            Assign { name, value } => {
                self.output.push_str(name);
                self.output.push_str(" = ");
                self.format_expr(value);
            }
            Binary { left, op, right } => {
                let needs_paren = matches!(op, BinaryOp::And | BinaryOp::Or);
                if needs_paren {
                    self.output.push('(');
                }
                self.format_expr(left);
                self.output.push(' ');
                self.output.push_str(self.binary_op_str(*op));
                self.output.push(' ');
                self.format_expr(right);
                if needs_paren {
                    self.output.push(')');
                }
            }
            Unary { op, operand } => {
                self.output.push_str(self.unary_op_str(*op));
                self.format_expr(operand);
            }
            Call { callee, args } => {
                self.format_expr(callee);
                self.output.push('(');
                for (i, arg) in args.iter().enumerate() {
                    self.format_expr(arg);
                    if i < args.len() - 1 {
                        self.output.push_str(", ");
                    }
                }
                self.output.push(')');
            }
            Array(elements) => {
                self.output.push('[');
                for (i, elem) in elements.iter().enumerate() {
                    self.format_expr(elem);
                    if i < elements.len() - 1 {
                        self.output.push_str(", ");
                    } else if self.config.trailing_comma && !elements.is_empty() {
                        self.output.push(',');
                    }
                }
                self.output.push(']');
            }
            Map(entries) => {
                self.output.push('{');
                for (i, (key, value)) in entries.iter().enumerate() {
                    self.format_expr(key);
                    self.output.push_str(": ");
                    self.format_expr(value);
                    if i < entries.len() - 1 {
                        self.output.push_str(", ");
                    } else if self.config.trailing_comma && !entries.is_empty() {
                        self.output.push(',');
                    }
                }
                self.output.push('}');
            }
            GetIndex { array, index } => {
                self.format_expr(array);
                self.output.push('[');
                self.format_expr(index);
                self.output.push(']');
            }
            SetIndex { array, index, op, value } => {
                self.format_expr(array);
                self.output.push('[');
                self.format_expr(index);
                self.output.push(']');
                if let Some(op) = op {
                    self.output.push(' ');
                    self.output.push_str(self.binary_op_str(*op));
                    self.output.push('=');
                } else {
                    self.output.push_str(" = ");
                }
                self.format_expr(value);
            }
            Conditional { condition, then_expr, else_expr } => {
                self.format_expr(condition);
                self.output.push_str(" ? ");
                self.format_expr(then_expr);
                self.output.push_str(" : ");
                self.format_expr(else_expr);
            }
            Await(expr) => {
                self.output.push_str("await ");
                self.format_expr(expr);
            }
            Spawn(expr) => {
                self.output.push_str("spawn(");
                self.format_expr(expr);
                self.output.push(')');
            }
            Grouping(expr) => {
                self.output.push('(');
                self.format_expr(expr);
                self.output.push(')');
            }
        }
    }

    fn format_value(&mut self, value: &Value) {
        match value {
            Value::Number(n) => {
                if n.fract() == 0.0 {
                    self.output.push_str(&format!("{:.0}", n));
                } else {
                    self.output.push_str(&n.to_string());
                }
            }
            Value::String(s) => {
                let quote = if self.config.single_quote { '\'' } else { '"' };
                self.output.push(quote);
                self.output.push_str(&escape_string(s, quote));
                self.output.push(quote);
            }
            Value::Boolean(b) => self.output.push_str(if *b { "true" } else { "false" }),
            Value::Null => self.output.push_str("null"),
            Value::Function(f) => {
                self.output.push_str(&format!("<fn {}>", f.name));
            }
            Value::Future(_) => self.output.push_str("<future>"),
            Value::Channel(_) => self.output.push_str("<channel>"),
            Value::Mutex(_) => self.output.push_str("<mutex>"),
            Value::ThreadHandle(_) => self.output.push_str("<thread_handle>"),
            Value::DbConnection(_) => self.output.push_str("<db_connection>"),
            Value::GuiWindow(_) => self.output.push_str("<gui_window>"),
            Value::Array(_) => self.output.push_str("<array>"),
            Value::Map(_) => self.output.push_str("<map>"),
        }
    }

    fn binary_op_str(&self, op: BinaryOp) -> &'static str {
        match op {
            BinaryOp::Add => "+",
            BinaryOp::Subtract => "-",
            BinaryOp::Multiply => "*",
            BinaryOp::Divide => "/",
            BinaryOp::Modulo => "%",
            BinaryOp::Equal => "==",
            BinaryOp::NotEqual => "!=",
            BinaryOp::Less => "<",
            BinaryOp::Greater => ">",
            BinaryOp::LessEqual => "<=",
            BinaryOp::GreaterEqual => ">=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
        }
    }

    fn unary_op_str(&self, op: UnaryOp) -> &'static str {
        match op {
            UnaryOp::Negate => "-",
            UnaryOp::Not => "not ",
        }
    }

    fn write_indent(&mut self) {
        let spaces = self.indent_level * self.config.indent_size;
        self.output.push_str(&" ".repeat(spaces));
    }

    fn newline(&mut self) {
        self.output.push('\n');
    }
}

fn escape_string(s: &str, quote: char) -> String {
    let mut result = s.replace('\\', "\\\\");
    if quote == '"' {
        result = result.replace('"', "\\\"");
    } else {
        result = result.replace('\'', "\\'");
    }
    result
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\t', "\\t")
}

pub fn format_source(source: &str, config: FormatConfig) -> Result<String, String> {
    let lexer = crate::lexer::Lexer::new(source);
    let tokens = lexer.tokenize().map_err(|e| e.message)?;
    let parser = crate::parser::Parser::new(tokens);
    let stmts = parser.parse().map_err(|e| e.message)?;
    let mut formatter = Formatter::new(config);
    Ok(formatter.format(&stmts))
}

pub fn format_file(path: &std::path::Path, config: FormatConfig) -> Result<(), String> {
    let source = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
    let formatted = format_source(&source, config)?;
    std::fs::write(path, formatted).map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_let() {
        let source = "let x=42";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert_eq!(formatted.trim(), "let x = 42");
    }

    #[test]
    fn test_format_function() {
        let source = "fn add(a,b){return a+b}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("fn add(a, b)"));
        assert!(formatted.contains("return a + b"));
    }

    #[test]
    fn test_format_if_else() {
        let source = "if x>0{print(x)}else{print(0)}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("if x > 0"));
        assert!(formatted.contains("else"));
    }

    #[test]
    fn test_format_while() {
        let source = "while x<10{x=x+1}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("while x < 10"));
    }

    #[test]
    fn test_format_for() {
        let source = "for i in arr{print(i)}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("for i in arr"));
    }

    #[test]
    fn test_format_binary_ops() {
        let source = "let x=1+2*3";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert_eq!(formatted.trim(), "let x = 1 + 2 * 3");
    }

    #[test]
    fn test_format_array() {
        let source = "let a=[1,2,3]";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert_eq!(formatted.trim(), "let a = [1, 2, 3]");
    }

    #[test]
    fn test_format_map() {
        let source = "let m={\"a\":1,b:2}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("\"a\": 1"));
        assert!(formatted.contains("\"b\": 2"));
    }

    #[test]
    fn test_indent_nested() {
        let source = "fn f(){if x>0{while y<10{y=y+1}}}";
        let formatted = format_source(source, FormatConfig::default()).unwrap();
        assert!(formatted.contains("    if"));
        assert!(formatted.contains("        while"));
    }
}