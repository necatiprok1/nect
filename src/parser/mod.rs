use crate::ast::*;
use crate::lexer::{Lexer, Token, TokenKind};

#[derive(Debug, Clone)]
pub struct ParseError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "parse error at line {}, col {}: {}", self.line, self.col, self.message)
    }
}

impl std::error::Error for ParseError {}

pub struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    pub fn new(tokens: Vec<Token>) -> Self {
        Self {
            tokens: fold_expressions_over_lines(tokens),
            pos: 0,
        }
    }
    pub fn parse(mut self) -> Result<Vec<Stmt>, ParseError> {
        let mut stmts = Vec::new();
        while !self.is_at_end() {
            self.consume_newlines();
            if self.is_at_end() {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        Ok(stmts)
    }

    fn is_at_end(&self) -> bool {
        matches!(self.peek().kind, TokenKind::EOF)
    }

    fn peek(&self) -> &Token {
        &self.tokens[self.pos]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.pos - 1]
    }

    fn advance(&mut self) -> &Token {
        if !self.is_at_end() {
            self.pos += 1;
        }
        self.previous()
    }

    fn matches(&mut self, kinds: &[TokenKind]) -> bool {
        for kind in kinds {
            if self.check_kind(kind) {
                self.advance();
                return true;
            }
        }
        false
    }

    fn check_kind(&self, kind: &TokenKind) -> bool {
        if self.is_at_end() {
            return false;
        }
        self.peek().kind == *kind
    }

    fn consume_newlines(&mut self) {
        while self.matches(&[TokenKind::Newline, TokenKind::Semicolon]) {}
    }

    fn expect(&mut self, kind: TokenKind, msg: &str) -> Result<&Token, ParseError> {
        if self.check_kind(&kind) {
            return Ok(self.advance());
        }
        let token = self.peek();
        Err(ParseError {
            message: format!("{} — found '{}'", msg, token_name(&token.kind)),
            line: token.line,
            col: token.col,
        })
    }

    fn parse_stmt(&mut self) -> Result<Stmt, ParseError> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Let => {
                self.advance();
                self.consume_newlines();
                let line = self.peek().line;
                let col = self.peek().col;
                let name = self.expect_identifier_at(line, col, "expected variable name")?;
                self.expect(TokenKind::Assign, "expected '=' after variable name")?;
                let value = self.parse_expr()?;
                self.consume_newlines();
                Ok(Stmt::Let { name, value })
            }
            TokenKind::Func => {
                self.advance();
                self.consume_newlines();
                let line = self.peek().line;
                let col = self.peek().col;
                let name = self.expect_identifier_at(line, col, "expected function name")?;
                self.expect(TokenKind::LeftParen, "expected '(' after function name")?;
                let mut params = Vec::new();
                if !self.check_kind(&TokenKind::RightParen) {
                    loop {
                        let param_line = self.peek().line;
                        let param_col = self.peek().col;
                        params.push(self.expect_identifier_at(param_line, param_col, "expected parameter name")?);
                        if !self.matches(&[TokenKind::Comma])
                            || self.check_kind(&TokenKind::RightParen)
                        {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RightParen, "expected ')' after parameters")?;
                let body = self.parse_block()?;
                Ok(Stmt::Function { name, params, body })
            }
            TokenKind::Async => {
                self.advance();
                self.consume_newlines();
                // Expect "fn" after "async"
                self.expect(TokenKind::Func, "expected 'fn' after 'async'")?;
                self.consume_newlines();
                let line = self.peek().line;
                let col = self.peek().col;
                let name = self.expect_identifier_at(line, col, "expected function name")?;
                self.expect(TokenKind::LeftParen, "expected '(' after function name")?;
                let mut params = Vec::new();
                if !self.check_kind(&TokenKind::RightParen) {
                    loop {
                        let param_line = self.peek().line;
                        let param_col = self.peek().col;
                        params.push(self.expect_identifier_at(param_line, param_col, "expected parameter name")?);
                        if !self.matches(&[TokenKind::Comma])
                            || self.check_kind(&TokenKind::RightParen)
                        {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RightParen, "expected ')' after parameters")?;
                let body = self.parse_block()?;
                Ok(Stmt::AsyncFunction { name, params, body })
            }
            TokenKind::If => {
                self.advance();
                let cond = self.parse_parenthesised_or_bare_condition("if")?;
                let then_branch = self.parse_block()?;
                // `else if` is an `else` whose branch is a single nested `if`,
                // so chains of conditions need no special handling later.
                let else_branch = if self.matches(&[TokenKind::Else]) {
                    if self.check_kind(&TokenKind::If) {
                        vec![self.parse_stmt()?]
                    } else {
                        self.parse_block()?
                    }
                } else {
                    Vec::new()
                };
                Ok(Stmt::If {
                    condition: cond,
                    then_branch,
                    else_branch,
                })
            }
            TokenKind::While => {
                self.advance();
                let cond = self.parse_parenthesised_or_bare_condition("while")?;
                let body = self.parse_block()?;
                Ok(Stmt::While {
                    condition: cond,
                    body,
                })
            }
            TokenKind::For => {
                self.advance();
                self.consume_newlines();
                let line = self.peek().line;
                let col = self.peek().col;
                let var_name = self.expect_identifier_at(line, col, "expected loop variable name")?;
                self.expect(TokenKind::In, "expected 'in' after loop variable")?;
                let iterable = self.parse_expr()?;
                let body = self.parse_block()?;
                Ok(Stmt::For {
                    var_name,
                    iterable,
                    body,
                })
            }
            TokenKind::Return => {
                self.advance();
                let value = if !self.check_kind(&TokenKind::Semicolon)
                    && !self.check_kind(&TokenKind::Newline)
                    && !self.is_at_end()
                    && !self.check_kind(&TokenKind::RightBrace)
                {
                    Some(self.parse_expr()?)
                } else {
                    None
                };
                self.consume_newlines();
                Ok(Stmt::Return(value))
            }
            TokenKind::Break => {
                self.advance();
                self.consume_newlines();
                Ok(Stmt::Break)
            }
            TokenKind::Continue => {
                self.advance();
                self.consume_newlines();
                Ok(Stmt::Continue)
            }
            TokenKind::LeftBrace => {
                let body = self.parse_block()?;
                Ok(Stmt::Block(body))
            }
            _ => {
                let expr = self.parse_expr()?;
                self.consume_newlines();
                Ok(Stmt::Expression(expr))
            }
        }
    }

    /// Parses an `if`/`while` condition, with or without parentheses.
    ///
    /// Both spellings are accepted because newcomers arrive from Python (no
    /// parentheses) and from C-family languages (parentheses), and arguing
    /// about which is right is not worth a syntax error. The expression ends
    /// at the `{` that starts the body, so `if x > 1 { ... }` and
    /// `if (x > 1) { ... }` mean the same thing.
    fn parse_parenthesised_or_bare_condition(&mut self, keyword: &str) -> Result<Expr, ParseError> {
        if self.check_kind(&TokenKind::LeftParen) {
            self.advance();
            let cond = self.parse_expr()?;
            self.expect(TokenKind::RightParen, &format!("expected ')' after the {keyword} condition"))?;
            return Ok(cond);
        }
        // Without an opening parenthesis the condition still stops before the
        // body brace, because `{` cannot appear inside an expression.
        let cond = self.parse_expr()?;
        Ok(cond)
    }

    fn expect_identifier_at(&mut self, line: usize, col: usize, msg: &str) -> Result<String, ParseError> {
        let token = self.peek();
        match &token.kind {
            TokenKind::Identifier(s) => {
                let name = s.clone();
                self.advance();
                Ok(name)
            }
            _ => Err(ParseError {
                message: format!("{} — found '{}'", msg, token_name(&token.kind)),
                line,
                col,
            }),
        }
    }

    fn parse_block(&mut self) -> Result<Vec<Stmt>, ParseError> {
        self.expect(TokenKind::LeftBrace, "expected '{' to start block")?;
        let mut stmts = Vec::new();
        while !self.check_kind(&TokenKind::RightBrace) && !self.is_at_end() {
            self.consume_newlines();
            if self.check_kind(&TokenKind::RightBrace) {
                break;
            }
            stmts.push(self.parse_stmt()?);
        }
        self.expect(TokenKind::RightBrace, "expected '}' to close block")?;
        Ok(stmts)
    }

    pub fn parse_expr(&mut self) -> Result<Expr, ParseError> {
        self.parse_assignment()
    }

    /// Whether the tokens from here on start an assignment.
    ///
    /// The target is a name optionally followed by `[...]` suffixes, so the
    /// operator can only be recognised after skipping the suffixes; a plain
    /// check of the next token would mistake `arr[i] + 1` for an assignment.
    fn starts_assignment(&self) -> bool {
        if !matches!(self.peek().kind, TokenKind::Identifier(_)) {
            return false;
        }
        let mut index = self.pos + 1;
        // `x.key` chains, like `x.key.sub = 1`; only a call parenthesis ends
        // the property chain (and with it the assignment form).
        while index < self.tokens.len()
            && matches!(self.tokens[index].kind, TokenKind::LeftBracket | TokenKind::Dot)
        {
            if self.tokens[index].kind == TokenKind::Dot {
                // Skip the dot and its property name.
                index += 2;
                continue;
            }
            let mut depth = 0usize;
            loop {
                match self.tokens.get(index).map(|token| &token.kind) {
                    Some(TokenKind::LeftBracket) => depth += 1,
                    Some(TokenKind::RightBracket) => {
                        depth -= 1;
                        index += 1;
                        if depth == 0 {
                            break;
                        }
                        continue;
                    }
                    Some(TokenKind::EOF) | None => return false,
                    _ => {}
                }
                index += 1;
            }
        }
        self.tokens
            .get(index)
            .is_some_and(|token| is_assignment_operator(&token.kind))
    }

    fn parse_assignment(&mut self) -> Result<Expr, ParseError> {
        if self.starts_assignment() {
            let lvalue = self.parse_index_lvalue()?;
            let op = compound_operator(&self.peek().kind);
            if op.is_none() {
                self.expect(TokenKind::Assign, "expected '=' after the assignment target")?;
            } else {
                self.advance();
            }
            let value = self.parse_assignment()?;
            match lvalue {
                Expr::Variable(name) => {
                    // `x += e` means `x = x + e`. Because the target is a plain
                    // name, the rewrite re-evaluates nothing.
                    let value = match op {
                        Some(op) => Expr::Binary {
                            left: Box::new(Expr::Variable(name.clone())),
                            op,
                            right: Box::new(value),
                        },
                        None => value,
                    };
                    return Ok(Expr::Assign {
                        name,
                        value: Box::new(value),
                    });
                }
                Expr::GetIndex { array, index } => {
                    // Here the index may be an arbitrary expression, so the
                    // operator travels with the node instead of being expanded.
                    return Ok(Expr::SetIndex {
                        array,
                        index,
                        op,
                        value: Box::new(value),
                    });
                }
                _ => {}
            }
        }
        // The conditional expression sits below `||` and is right-associative,
        // so `a ? b : c ? d : e` parses as `a ? b : (c ? d : e)`.
        let expr = self.parse_or()?;
        if self.matches(&[TokenKind::Question]) {
            let then_expr = self.parse_expr()?;
            self.expect(TokenKind::Colon, "expected ':' in a conditional expression")?;
            let else_expr = self.parse_assignment()?;
            return Ok(Expr::Conditional {
                condition: Box::new(expr),
                then_expr: Box::new(then_expr),
                else_expr: Box::new(else_expr),
            });
        }
        Ok(expr)
    }

    fn parse_index_lvalue(&mut self) -> Result<Expr, ParseError> {
        let token = self.peek();
        let line = token.line;
        let col = token.col;
        match &token.kind {
            TokenKind::Identifier(s) => {
                let mut target = Expr::Variable(s.clone());
                self.advance();
                // `matrix[i][j].field = value` chains one suffix at a time.
                loop {
                    if self.check_kind(&TokenKind::LeftBracket) {
                        self.advance();
                        let index = self.parse_expr()?;
                        self.expect(TokenKind::RightBracket, "expected ']' after index")?;
                        target = Expr::GetIndex {
                            array: Box::new(target),
                            index: Box::new(index),
                        };
                    } else if self.check_kind(&TokenKind::Dot) {
                        // Property write: d.key = v is d["key"] = v.
                        self.advance();
                        let name = self.expect_identifier_at(
                            self.peek().line,
                            self.peek().col,
                            "expected a property name after '.'",
                        )?;
                        target = Expr::GetIndex {
                            array: Box::new(target),
                            index: Box::new(Expr::Literal(Value::String(name))),
                        };
                    } else {
                        break;
                    }
                }
                Ok(target)
            }
            _ => Err(ParseError {
                message: format!("expected an assignment target, found '{}'", token_name(&token.kind)),
                line,
                col,
            }),
        }
    }

    fn parse_or(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_and()?;
        while self.matches(&[TokenKind::Or, TokenKind::OrWord]) {
            let right = self.parse_and()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::Or,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_and(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_equality()?;
        while self.matches(&[TokenKind::And, TokenKind::AndWord]) {
            let right = self.parse_equality()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op: BinaryOp::And,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_equality(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_comparison()?;
        while self.matches(&[TokenKind::Equal, TokenKind::NotEqual]) {
            let op = match self.previous().kind {
                TokenKind::Equal => BinaryOp::Equal,
                TokenKind::NotEqual => BinaryOp::NotEqual,
                _ => unreachable!(),
            };
            let right = self.parse_comparison()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_comparison(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_term()?;
        while self.matches(&[
            TokenKind::Less,
            TokenKind::Greater,
            TokenKind::LessEqual,
            TokenKind::GreaterEqual,
        ]) {
            let op = match self.previous().kind {
                TokenKind::Less => BinaryOp::Less,
                TokenKind::Greater => BinaryOp::Greater,
                TokenKind::LessEqual => BinaryOp::LessEqual,
                TokenKind::GreaterEqual => BinaryOp::GreaterEqual,
                _ => unreachable!(),
            };
            let right = self.parse_term()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_term(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_factor()?;
        while self.matches(&[TokenKind::Plus, TokenKind::Minus]) {
            let op = match self.previous().kind {
                TokenKind::Plus => BinaryOp::Add,
                TokenKind::Minus => BinaryOp::Subtract,
                _ => unreachable!(),
            };
            let right = self.parse_factor()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_factor(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_unary()?;
        while self.matches(&[TokenKind::Star, TokenKind::Slash, TokenKind::Percent]) {
            let op = match self.previous().kind {
                TokenKind::Star => BinaryOp::Multiply,
                TokenKind::Slash => BinaryOp::Divide,
                TokenKind::Percent => BinaryOp::Modulo,
                _ => unreachable!(),
            };
            let right = self.parse_unary()?;
            expr = Expr::Binary {
                left: Box::new(expr),
                op,
                right: Box::new(right),
            };
        }
        Ok(expr)
    }

    fn parse_unary(&mut self) -> Result<Expr, ParseError> {
        if self.matches(&[TokenKind::Bang, TokenKind::Minus, TokenKind::NotWord]) {
            let op = match self.previous().kind {
                TokenKind::Bang | TokenKind::NotWord => UnaryOp::Not,
                TokenKind::Minus => UnaryOp::Negate,
                _ => unreachable!(),
            };
            let operand = self.parse_unary()?;
            return Ok(Expr::Unary { op, operand: Box::new(operand) });
        }
        self.parse_call()
    }

    fn parse_call(&mut self) -> Result<Expr, ParseError> {
        let mut expr = self.parse_primary()?;
        loop {
            // `d.key = value` and `d.key op= value` are property writes on a
            // map; the assignment loop below handles the `=` once the left
            // side has been shaped into `d["key"]` here.
            if self.check_kind(&TokenKind::LeftParen) {
                self.advance();
                let mut args = Vec::new();
                if !self.check_kind(&TokenKind::RightParen) {
                    loop {
                        args.push(self.parse_expr()?);
                        // A trailing comma before `)` is allowed.
                        if !self.matches(&[TokenKind::Comma])
                            || self.check_kind(&TokenKind::RightParen)
                        {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RightParen, "expected ')' after arguments")?;
                expr = Expr::Call {
                    callee: Box::new(expr),
                    args,
                };
            } else if self.check_kind(&TokenKind::LeftBracket) {
                self.advance();
                let index = self.parse_expr()?;
                self.expect(TokenKind::RightBracket, "expected ']' after index")?;
                expr = Expr::GetIndex {
                    array: Box::new(expr),
                    index: Box::new(index),
                };
            } else if self.check_kind(&TokenKind::Dot) {
                // Two things can follow a `.`:
                //   `x.m(...)`  — method-call sugar for `m(x, ...)`
                //   `x.m` / `x.m = v` — property read/write on a map,
                // shorthand for `x["m"]` and `x["m"] = v`.
                // The next token after the name decides which form it is.
                self.advance();
                let line = self.peek().line;
                let col = self.peek().col;
                let method = self.expect_identifier_at(line, col, "expected a method name after '.'")?;
                if !self.check_kind(&TokenKind::LeftParen) {
                    // Property access: x.m is x["m"].
                    expr = Expr::GetIndex {
                        array: Box::new(expr),
                        index: Box::new(Expr::Literal(Value::String(method))),
                    };
                    continue;
                }
                let mut arguments = vec![expr];
                if self.check_kind(&TokenKind::LeftParen) {
                    self.advance();
                    if !self.check_kind(&TokenKind::RightParen) {
                        loop {
                            arguments.push(self.parse_expr()?);
                            if !self.matches(&[TokenKind::Comma])
                                || self.check_kind(&TokenKind::RightParen)
                            {
                                break;
                            }
                        }
                    }
                    self.expect(TokenKind::RightParen, "expected ')' after arguments")?;
                }
                expr = Expr::Call {
                    callee: Box::new(Expr::Variable(method)),
                    args: arguments,
                };
            } else {
                break;
            }
        }
        Ok(expr)
    }

    /// Turns a string literal with `${expr}` markers into a `concat(...)`
    /// expression that splices the interpolated values in.
    ///
    /// The lexer wrote each interpolation as a marker, the expression's source
    /// text, and an end marker, all inside the literal. Plain literal text
    /// becomes string arguments; each marked expression is lexed and parsed in
    /// place, so `"${a + b}"` is exactly `concat(a + b)` and a mistake inside
    /// the braces is reported at its own position. A literal with no markers
    /// stays a plain string, so nothing changes for programs that do not use
    /// the feature.
    fn expand_interpolation(&mut self, literal: &str, line: usize, col: usize) -> Result<Expr, ParseError> {
        const MARK: char = '\u{1}';
        const HEAD: &str = "NECT-INTERP";
        if !literal.contains(MARK) {
            return Ok(Expr::Literal(Value::String(literal.to_string())));
        }

        // Each interpolation sits in the literal as MARK HEAD MARK expression
        // MARK (see the lexer). This loop consumes one interpolation per pass:
        // everything before it is literal text, the expression inside is lexed
        // and parsed in place, and parsing continues after its end marker.
        let mut pieces: Vec<Expr> = Vec::new();
        let mut plain = String::new();
        let mut rest = literal;
        while let Some(start) = rest.find(MARK) {
            let (before, after) = rest.split_at(start);
            plain.push_str(before);
            let after = &after[MARK.len_utf8()..];
            if !plain.is_empty() {
                pieces.push(Expr::Literal(Value::String(std::mem::take(&mut plain))));
            }
            let Some(body) = after.strip_prefix(HEAD) else {
                return Err(ParseError {
                    message: "malformed string interpolation".to_string(),
                    line,
                    col,
                });
            };
            let body = body.strip_prefix(MARK).unwrap_or(body);
            let Some(end) = body.find(MARK) else {
                return Err(ParseError {
                    message: "malformed string interpolation".to_string(),
                    line,
                    col,
                });
            };
            let expression = &body[..end];
            rest = &body[end + MARK.len_utf8()..];
            if expression.trim().is_empty() {
                return Err(ParseError {
                    message: "empty interpolation: put an expression inside '${}'".to_string(),
                    line,
                    col,
                });
            }
            let tokens = Lexer::new(expression)
                .tokenize()
                .map_err(|error| ParseError {
                    message: format!("in '${{{}}}' — {}", expression.trim(), error.message),
                    line,
                    col,
                })?;
            let mut nested = Parser::new(tokens);
            let parsed = nested.parse_expr().map_err(|error| ParseError {
                message: format!("in '${{{}}}' — {}", expression.trim(), error.message),
                line,
                col,
            })?;
            pieces.push(parsed);
        }
        plain.push_str(rest);
        if !plain.is_empty() {
            pieces.push(Expr::Literal(Value::String(plain)));
        }
        if pieces.is_empty() {
            return Ok(Expr::Literal(Value::String(String::new())));
        }
        if pieces.len() == 1 {
            return Ok(pieces.pop().expect("one piece"));
        }
        Ok(Expr::Call {
            callee: Box::new(Expr::Variable("concat".to_string())),
            args: pieces,
        })
    }

    fn parse_primary(&mut self) -> Result<Expr, ParseError> {
        let token = self.peek();
        let line = token.line;
        let col = token.col;
        match &token.kind {
            TokenKind::Number(n) => {
                let n = *n;
                self.advance();
                Ok(Expr::Literal(Value::Number(n)))
            }
            TokenKind::Str(s) => {
                let s = s.clone();
                self.advance();
                self.expand_interpolation(&s, line, col)
            }
            TokenKind::True => {
                self.advance();
                Ok(Expr::Literal(Value::Boolean(true)))
            }
            TokenKind::False => {
                self.advance();
                Ok(Expr::Literal(Value::Boolean(false)))
            }
            TokenKind::Null => {
                self.advance();
                Ok(Expr::Literal(Value::Null))
            }
            TokenKind::LeftBracket => {
                self.advance();
                let mut elements = Vec::new();
                if !self.check_kind(&TokenKind::RightBracket) {
                    loop {
                        elements.push(self.parse_expr()?);
                        // A trailing comma before `]` is allowed.
                        if !self.matches(&[TokenKind::Comma])
                            || self.check_kind(&TokenKind::RightBracket)
                        {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RightBracket, "expected ']' after array elements")?;
                Ok(Expr::Array(elements))
            }
            TokenKind::LeftBrace => {
                // A `{` in expression position is a map literal, so `{ }` as a
                // statement is still a bare block (statement position wins).
                self.advance();
                let mut entries: Vec<(Expr, Expr)> = Vec::new();
                if !self.check_kind(&TokenKind::RightBrace) {
                    loop {
                        let key = self.parse_map_key()?;
                        self.expect(TokenKind::Colon, "expected ':' after a map key")?;
                        let value = self.parse_expr()?;
                        entries.push((key, value));
                        // A trailing comma before `}` is allowed.
                        if !self.matches(&[TokenKind::Comma])
                            || self.check_kind(&TokenKind::RightBrace)
                        {
                            break;
                        }
                    }
                }
                self.expect(TokenKind::RightBrace, "expected '}' after map entries")?;
                Ok(Expr::Map(entries))
            }
            TokenKind::Identifier(_) => {
                let name = match &self.peek().kind {
                    TokenKind::Identifier(s) => s.clone(),
                    _ => unreachable!(),
                };
                self.advance();
                Ok(Expr::Variable(name))
            }
            TokenKind::LeftParen => {
                self.advance();
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RightParen, "expected ')' after expression")?;
                Ok(Expr::Grouping(Box::new(expr)))
            }
            TokenKind::Await => {
                self.advance();
                let expr = self.parse_primary()?;
                Ok(Expr::Await(Box::new(expr)))
            }
            TokenKind::Spawn => {
                self.advance();
                self.expect(TokenKind::LeftParen, "expected '(' after 'spawn'")?;
                let expr = self.parse_expr()?;
                self.expect(TokenKind::RightParen, "expected ')' after spawn argument")?;
                Ok(Expr::Spawn(Box::new(expr)))
            }
            _ => Err(ParseError {
                message: format!("expected an expression, found '{}'", token_name(&token.kind)),
                line,
                col,
            }),
        }
    }

    /// One `{key: ...}` key. A bare identifier is the string of the same name
    /// (`{name: "Ada"}` is `{"name": "Ada"}`); anything else must be an
    /// expression that evaluates to a string, number, or boolean.
    fn parse_map_key(&mut self) -> Result<Expr, ParseError> {
        if let TokenKind::Identifier(name) = &self.peek().kind {
            let name = name.clone();
            self.advance();
            return Ok(Expr::Literal(Value::String(name)));
        }
        self.parse_expr()
    }
}

/// Drops the newlines inside `(...)` and `[...]`, so an expression may be laid
/// out over several lines:
///
/// ```text
/// let total = sum([
///     1,
///     2,
/// ])
/// ```
///
/// Newlines outside brackets keep separating statements, and the inside of a
/// `{ ... }` block is untouched — those newlines are statement boundaries.
fn fold_expressions_over_lines(tokens: Vec<Token>) -> Vec<Token> {
    let mut depth = 0usize;
    // A `{` in *expression* position is a map literal and may span lines, but
    // in statement position it is a bare block whose newlines separate
    // statements. A map literal can only start after `= ( [ , : ?` or an
    // operator — never at the start of a statement — so the tracker below
    // treats `{` as depth-increasing only while a parenthesised/bracketed
    // expression is already open (which is where maps appear mid-expression).
    // A top-level `{ ... }` keeps statement semantics.
    let mut in_expr_braces = Vec::new();
    let mut folded = Vec::with_capacity(tokens.len());
    let mut previous: Option<&TokenKind> = None;
    for token in &tokens {
        match token.kind {
            TokenKind::LeftParen | TokenKind::LeftBracket => {
                depth += 1;
                folded.push(token.clone());
            }
            TokenKind::LeftBrace if depth > 0 || expression_continues(previous) => {
                // Map literal inside an expression (e.g. after `=` or `return`
                // is not an expression opener, so only depth/`=`-like leads).
                depth += 1;
                in_expr_braces.push(true);
                folded.push(token.clone());
            }
            TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                depth = depth.saturating_sub(1);
                if token.kind == TokenKind::RightBrace && in_expr_braces.pop().is_none() {
                    // A bare block's closing brace: not expression depth.
                    depth = depth.saturating_sub(0);
                }
                folded.push(token.clone());
            }
            TokenKind::Newline if depth > 0 => {}
            _ => folded.push(token.clone()),
        }
        previous = Some(&token.kind);
    }
    folded
}

/// Whether a `{` directly after this token is a map literal (an expression
/// continuation) rather than the start of a bare block.
fn expression_continues(previous: Option<&TokenKind>) -> bool {
    matches!(
        previous,
        Some(TokenKind::Assign)
            | Some(TokenKind::PlusAssign)
            | Some(TokenKind::MinusAssign)
            | Some(TokenKind::StarAssign)
            | Some(TokenKind::SlashAssign)
            | Some(TokenKind::PercentAssign)
            | Some(TokenKind::Comma)
            | Some(TokenKind::Colon)
            | Some(TokenKind::Question)
            | Some(TokenKind::LeftParen)
            | Some(TokenKind::LeftBracket)
            | Some(TokenKind::Equal)
            | Some(TokenKind::NotEqual)
            | Some(TokenKind::And)
            | Some(TokenKind::Or)
            | Some(TokenKind::AndWord)
            | Some(TokenKind::OrWord)
    )
}

/// Assignment operators that have a compound form (`x += 1`).
fn is_assignment_operator(kind: &TokenKind) -> bool {
    matches!(
        kind,
        TokenKind::Assign
            | TokenKind::PlusAssign
            | TokenKind::MinusAssign
            | TokenKind::StarAssign
            | TokenKind::SlashAssign
            | TokenKind::PercentAssign
    )
}

/// The arithmetic operator behind a compound assignment token.
fn compound_operator(kind: &TokenKind) -> Option<BinaryOp> {
    match kind {
        TokenKind::PlusAssign => Some(BinaryOp::Add),
        TokenKind::MinusAssign => Some(BinaryOp::Subtract),
        TokenKind::StarAssign => Some(BinaryOp::Multiply),
        TokenKind::SlashAssign => Some(BinaryOp::Divide),
        TokenKind::PercentAssign => Some(BinaryOp::Modulo),
        _ => None,
    }
}

/// How a token is described inside an error message.
///
/// Callers wrap the result in quotes, so the descriptions carry none of their
/// own: a message reads `found '*'`, not `found ''*''`.
fn token_name(kind: &TokenKind) -> String {
    match kind {
        TokenKind::EOF => "end of input".to_string(),
        TokenKind::Newline => "newline".to_string(),
        TokenKind::Identifier(s) => format!("identifier {}", s),
        TokenKind::Number(n) => format!("number {}", n),
        TokenKind::Str(_) => "string".to_string(),
        TokenKind::Let => "keyword let".to_string(),
        TokenKind::Func => "keyword fn".to_string(),
        TokenKind::Async => "keyword async".to_string(),
        TokenKind::Await => "keyword await".to_string(),
        TokenKind::Spawn => "keyword spawn".to_string(),
        TokenKind::If => "keyword if".to_string(),
        TokenKind::Else => "keyword else".to_string(),
        TokenKind::While => "keyword while".to_string(),
        TokenKind::For => "keyword for".to_string(),
        TokenKind::In => "keyword in".to_string(),
        TokenKind::Return => "keyword return".to_string(),
        TokenKind::Break => "keyword break".to_string(),
        TokenKind::Continue => "keyword continue".to_string(),
        TokenKind::True => "keyword true".to_string(),
        TokenKind::False => "keyword false".to_string(),
        TokenKind::Null => "keyword null".to_string(),
        TokenKind::AndWord => "keyword and".to_string(),
        TokenKind::OrWord => "keyword or".to_string(),
        TokenKind::NotWord => "keyword not".to_string(),
        TokenKind::Dot => ".".to_string(),
        TokenKind::Plus => "+".to_string(),
        TokenKind::Minus => "-".to_string(),
        TokenKind::Star => "*".to_string(),
        TokenKind::Slash => "/".to_string(),
        TokenKind::Percent => "%".to_string(),
        TokenKind::Assign => "=".to_string(),
        TokenKind::PlusAssign => "+=".to_string(),
        TokenKind::MinusAssign => "-=".to_string(),
        TokenKind::StarAssign => "*=".to_string(),
        TokenKind::SlashAssign => "/=".to_string(),
        TokenKind::PercentAssign => "%=".to_string(),
        TokenKind::Equal => "==".to_string(),
        TokenKind::NotEqual => "!=".to_string(),
        TokenKind::Less => "<".to_string(),
        TokenKind::Greater => ">".to_string(),
        TokenKind::LessEqual => "<=".to_string(),
        TokenKind::GreaterEqual => ">=".to_string(),
        TokenKind::Bang => "!".to_string(),
        TokenKind::And => "&&".to_string(),
        TokenKind::Or => "||".to_string(),
        TokenKind::LeftParen => "(".to_string(),
        TokenKind::RightParen => ")".to_string(),
        TokenKind::LeftBrace => "{".to_string(),
        TokenKind::RightBrace => "}".to_string(),
        TokenKind::LeftBracket => "[".to_string(),
        TokenKind::RightBracket => "]".to_string(),
        TokenKind::Question => "?".to_string(),
        TokenKind::Colon => ":".to_string(),
        TokenKind::Comma => ",".to_string(),
        TokenKind::Semicolon => ";".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(source: &str) -> Result<Vec<Stmt>, ParseError> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        Parser::new(tokens).parse()
    }

    use crate::lexer::Lexer;

    #[test]
    fn test_parse_print_statement() {
        let stmts = parse("print(\"hello\")").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_let_statement() {
        let stmts = parse("let x = 42").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::Let { .. }));
    }

    #[test]
    fn test_parse_let_string() {
        let stmts = parse("let name = \"nect\"").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_arithmetic() {
        let stmts = parse("print(1 + 2 * 3)").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_grouping() {
        let stmts = parse("print((1 + 2) * 3)").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_comparison() {
        let stmts = parse("1 < 2").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_equality() {
        let stmts = parse("1 == 1").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_unary() {
        let stmts = parse("print(-5)").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_function_definition() {
        let stmts = parse("fn add(a, b) {\n    return a + b\n}").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::Function { .. }));
    }

    #[test]
    fn test_parse_if_statement() {
        let stmts = parse("if (1 < 2) {\n    print(\"yes\")\n}").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_if_else() {
        let stmts = parse("if (true) {\n    print(\"a\")\n} else {\n    print(\"b\")\n}").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_while() {
        let stmts = parse("while (false) {\n    print(\"loop\")\n}").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(stmts[0], Stmt::While { .. }));
    }

    #[test]
    fn test_parse_return() {
        let stmts = parse("return 42").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_return_void() {
        let stmts = parse("return").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_multiple_statements() {
        let stmts = parse("let x = 1\nlet y = 2\nprint(x)").unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn test_parse_function_call() {
        let stmts = parse("greet(\"hello\", \"world\")").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_comments_ignored() {
        let stmts = parse("// comment\nlet x = 1").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_syntax_error_missing_paren() {
        let result = parse("print(\"hello\"");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_and() {
        let stmts = parse("true && false").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_or() {
        let stmts = parse("true || false").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_logical_precedence() {
        let stmts = parse("true || false && false").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_syntax_error_missing_brace() {
        let result = parse("if (true) {\n    print(\"yes\")");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_assignment() {
        let stmts = parse("let x = 1\nx = 2").unwrap();
        assert_eq!(stmts.len(), 2);
    }

    #[test]
    fn test_parse_break_and_continue() {
        let stmts = parse("while (true) {\n    if (a) {\n        break\n    }\n    continue\n}").unwrap();
        let Stmt::While { body, .. } = &stmts[0] else {
            panic!("expected a while loop");
        };
        let Stmt::If { then_branch, .. } = &body[0] else {
            panic!("expected an if");
        };
        assert!(matches!(then_branch[0], Stmt::Break));
        assert!(matches!(body[1], Stmt::Continue));
    }

    #[test]
    fn test_parse_else_if_chain() {
        let stmts = parse("if (a) {\n    print(1)\n} else if (b) {\n    print(2)\n} else {\n    print(3)\n}").unwrap();
        let Stmt::If { else_branch, .. } = &stmts[0] else {
            panic!("expected an if");
        };
        // `else if` is one nested `if` statement in the else branch.
        assert_eq!(else_branch.len(), 1);
        assert!(matches!(else_branch[0], Stmt::If { .. }));
        let Stmt::If {
            else_branch: inner_else,
            ..
        } = &else_branch[0]
        else {
            unreachable!();
        };
        assert_eq!(inner_else.len(), 1);
    }

    #[test]
    fn test_parse_compound_assignment_desugars_to_binary() {
        let stmts = parse("let x = 1\nx += 2").unwrap();
        let Stmt::Expression(Expr::Assign { value, .. }) = &stmts[1] else {
            panic!("expected an assignment");
        };
        assert!(matches!(
            **value,
            Expr::Binary {
                op: BinaryOp::Add,
                ..
            }
        ));
    }

    #[test]
    fn test_parse_index_compound_assignment_keeps_the_operator() {
        let stmts = parse("let a = [1]\na[0] -= 1").unwrap();
        let Stmt::Expression(Expr::SetIndex { op, .. }) = &stmts[1] else {
            panic!("expected an index assignment");
        };
        assert_eq!(*op, Some(BinaryOp::Subtract));
    }

    #[test]
    fn test_parse_index_expression_is_not_an_assignment() {
        // `a[0] + a[1]` starts like an assignment target; the parser must look
        // past the index before deciding it is arithmetic.
        let stmts = parse("print(a[0] + a[1])").unwrap();
        assert_eq!(stmts.len(), 1);
        assert!(matches!(&stmts[0], Stmt::Expression(Expr::Call { .. })));
    }

    #[test]
    fn test_parse_chained_index_assignment() {
        let stmts = parse("m[0][1] = 5").unwrap();
        let Stmt::Expression(Expr::SetIndex { array, .. }) = &stmts[0] else {
            panic!("expected an index assignment");
        };
        assert!(matches!(**array, Expr::GetIndex { .. }));
    }

    #[test]
    fn test_parse_modulo_and_ternary() {
        let stmts = parse("print(7 % 3)\nlet s = true ? \"a\" : \"b\"").unwrap();
        assert_eq!(stmts.len(), 2);
        let Stmt::Let { value, .. } = &stmts[1] else {
            panic!("expected a let");
        };
        assert!(matches!(value, Expr::Conditional { .. }));
    }

    #[test]
    fn test_parse_ternary_is_right_associative() {
        let stmts = parse("let x = a ? 1 : b ? 2 : 3").unwrap();
        let Stmt::Let { value, .. } = &stmts[0] else {
            panic!("expected a let");
        };
        let Expr::Conditional { else_expr, .. } = value else {
            panic!("expected a conditional");
        };
        assert!(matches!(**else_expr, Expr::Conditional { .. }));
    }

    #[test]
    fn test_parse_expressions_span_lines_inside_brackets() {
        let stmts = parse("let a = [\n    1,\n    2,\n]\nfn f(\n    x,\n) {\n    return x\n}\nprint(f(\n    sum(a),\n))\n").unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn test_newlines_still_separate_statements_outside_brackets() {
        // The folding pass must not merge statements outside `(...)`/`[...]`.
        let stmts = parse("let a = 1\nlet b = 2\nprint(a + b)").unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn test_parse_trailing_commas() {
        let stmts = parse("fn f(a, b,) {\n    return a\n}\nlet x = f(1, 2,)\nlet a = [1, 2,]").unwrap();
        assert_eq!(stmts.len(), 3);
    }

    #[test]
    fn test_parse_ternary_without_colon_is_an_error() {
        assert!(parse("let x = true ? 1").is_err());
    }

    #[test]
    fn test_parse_interpolation_becomes_concat() {
        // "hi ${name}!" desugars to concat("hi ", name, "!").
        let stmts = parse("let name = \"x\"\nprint(\"hi ${name}!\")").unwrap();
        let Stmt::Expression(Expr::Call { args, .. }) = &stmts[1] else {
            panic!("expected a call");
        };
        let Expr::Call { callee, args: pieces, .. } = &args[0] else {
            panic!("expected the interpolated string to become a concat call");
        };
        assert!(matches!(&**callee, Expr::Variable(name) if name == "concat"));
        assert_eq!(pieces.len(), 3);
    }

    #[test]
    fn test_parse_interpolation_with_expression() {
        // Braces nest: the ${...} placeholder holds a full expression.
        let stmts = parse("print(\"sum: ${1 + 2}\")").unwrap();
        let Stmt::Expression(Expr::Call { args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        let Expr::Call { args: pieces, .. } = &args[0] else {
            panic!("expected the interpolated string to become a concat call");
        };
        let Expr::Binary { op: BinaryOp::Add, .. } = &pieces[1] else {
            panic!("expected the arithmetic inside the placeholder");
        };
        assert_eq!(pieces.len(), 2);
    }

    #[test]
    fn test_parse_keyword_operators() {
        let stmts = parse("print(a and b or not c)").unwrap();
        let Stmt::Expression(Expr::Call { args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        // `or` binds loosest: or(a, and(b, not c))
        let Expr::Binary { op: BinaryOp::Or, left, right } = &args[0] else {
            panic!("expected an or");
        };
        assert!(matches!(**right, Expr::Unary { op: UnaryOp::Not, .. }));
        assert!(matches!(**left, Expr::Binary { op: BinaryOp::And, .. }));
    }

    #[test]
    fn test_parse_method_call_desugars_to_builtin_call() {
        let stmts = parse("print(items.len())").unwrap();
        let Stmt::Expression(Expr::Call { args: print_args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        let Expr::Call { callee, args, .. } = &print_args[0] else {
            panic!("expected the method call inside print");
        };
        assert!(matches!(&**callee, Expr::Variable(name) if name == "len"));
        assert_eq!(args.len(), 1);
    }

    #[test]
    fn test_parse_method_call_with_arguments() {
        let stmts = parse("print(s.replace(\"a\", \"b\"))").unwrap();
        let Stmt::Expression(Expr::Call { args: print_args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        let Expr::Call { callee, args, .. } = &print_args[0] else {
            panic!("expected the method call inside print");
        };
        assert!(matches!(&**callee, Expr::Variable(name) if name == "replace"));
        assert_eq!(args.len(), 3);
    }

    #[test]
    fn test_parse_chained_method_calls() {
        let stmts = parse("print(\"  a b  \".trim().upper())").unwrap();
        let Stmt::Expression(Expr::Call { args: print_args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        // The outermost chain link is `upper`, wrapping `trim`.
        let Expr::Call { callee, args, .. } = &print_args[0] else {
            panic!("expected the chained call");
        };
        assert!(matches!(&**callee, Expr::Variable(name) if name == "upper"));
        let Expr::Call { callee: inner, .. } = &args[0] else {
            panic!("expected the inner call");
        };
        assert!(matches!(&**inner, Expr::Variable(name) if name == "trim"));
    }

    #[test]
    fn test_parse_if_without_parentheses() {
        let stmts = parse("if x > 1 {\n    print(\"yes\")\n}").unwrap();
        assert!(matches!(stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_while_without_parentheses() {
        let stmts = parse("while i < 3 {\n    i += 1\n}").unwrap();
        assert!(matches!(stmts[0], Stmt::While { .. }));
    }

    #[test]
    fn test_parse_else_if_without_parentheses() {
        let stmts = parse("if a {\n    print(1)\n} else if b {\n    print(2)\n}").unwrap();
        assert!(matches!(stmts[0], Stmt::If { .. }));
    }

    #[test]
    fn test_parse_hash_comment() {
        let stmts = parse("# heading\nlet x = 1").unwrap();
        assert_eq!(stmts.len(), 1);
    }

    #[test]
    fn test_parse_loose_not_at_statement_start() {
        let stmts = parse("print(not true)").unwrap();
        let Stmt::Expression(Expr::Call { args, .. }) = &stmts[0] else {
            panic!("expected a call");
        };
        assert!(matches!(args[0], Expr::Unary { op: UnaryOp::Not, .. }));
    }
}
