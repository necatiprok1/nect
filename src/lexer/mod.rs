use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum TokenKind {
    Number(f64),
    Str(String),
    Identifier(String),

    Let,
    Func,
    Async,
    Extern,
    Await,
    Spawn,
    If,
    Else,
    While,
    For,
    In,
    Return,
    Break,
    Continue,
    True,
    False,
    Null,

    /// `and` / `or` / `not` — the word spellings of `&&` / `||` / `!`.
    AndWord,
    OrWord,
    NotWord,

    Dot,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Assign,
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Equal,
    NotEqual,
    Less,
    Greater,
    LessEqual,
    GreaterEqual,
    Bang,
    And,
    Or,

    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    LeftBracket,
    RightBracket,
    Question,
    Colon,
    Comma,
    Semicolon,
    Newline,

    EOF,
}

#[derive(Debug, Clone)]
pub struct Token {
    pub kind: TokenKind,
    pub start: usize,
    pub end: usize,
    pub line: usize,
    pub col: usize,
}

#[derive(Debug, Clone)]
pub struct LexError {
    pub message: String,
    pub line: usize,
    pub col: usize,
}

impl fmt::Display for LexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "lex error at line {}, col {}: {}",
            self.line, self.col, self.message
        )
    }
}

/// Whether `name` is an identifier the lexer would produce, as opposed to a
/// reserved word.
///
/// This is the single definition of "what a name may be": the scanner uses the
/// same rule, so a tool that validates a name (rename, for instance) cannot
/// accept something the language would then refuse to parse. Identifiers are
/// ASCII-only, which is a deliberate choice — it keeps the scanner byte-wise and
/// means a name is always the same length as its source text.
pub fn is_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    let Some(first) = bytes.next() else {
        return false;
    };
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    if !bytes.all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return false;
    }
    !is_reserved_word(name)
}

/// The reserved words, which the scanner turns into their own tokens and which
/// therefore cannot be used as names.
pub fn is_reserved_word(name: &str) -> bool {
    matches!(
        name,
        "let"
            | "fn"
            | "async"
            | "extern"
            | "await"
            | "spawn"
            | "if"
            | "else"
            | "while"
            | "for"
            | "in"
            | "return"
            | "break"
            | "continue"
            | "true"
            | "false"
            | "null"
            | "and"
            | "or"
            | "not"
    )
}

impl std::error::Error for LexError {}

pub struct Lexer<'a> {
    source: &'a [u8],
    pos: usize,
    line: usize,
    col: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            pos: 0,
            line: 1,
            col: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexError> {
        let mut tokens = Vec::new();
        loop {
            if self.pos >= self.source.len() {
                break;
            }
            self.skip_whitespace_and_comments();
            if self.pos >= self.source.len() {
                break;
            }
            let start = self.pos;
            let start_line = self.line;
            let start_col = self.col;
            let kind = self.read_token()?;
            let end = self.pos;
            tokens.push(Token {
                kind,
                start,
                end,
                line: start_line,
                col: start_col,
            });
        }
        tokens.push(Token {
            kind: TokenKind::EOF,
            start: self.pos,
            end: self.pos,
            line: self.line,
            col: self.col,
        });
        Ok(tokens)
    }

    fn skip_whitespace_and_comments(&mut self) {
        loop {
            match self.source.get(self.pos) {
                Some(b' ') | Some(b'\t') | Some(b'\r') => {
                    self.advance_simple();
                }
                Some(b'/') if self.peek_at(1) == Some(b'/') => {
                    while self.pos < self.source.len() && self.source[self.pos] != b'\n' {
                        self.advance_simple();
                    }
                }
                Some(b'/') if self.peek_at(1) == Some(b'*') => {
                    self.advance_simple();
                    self.advance_simple();
                    while self.pos < self.source.len() {
                        if self.source[self.pos] == b'\n' {
                            self.advance_simple();
                            self.line += 1;
                            self.col = 1;
                        } else if self.source[self.pos] == b'*' && self.peek_at(1) == Some(b'/') {
                            self.advance_simple();
                            self.advance_simple();
                            break;
                        } else {
                            self.advance_simple();
                        }
                    }
                    if self.pos >= self.source.len() {
                        break;
                    }
                }
                _ => break,
            }
        }
    }

    fn advance_simple(&mut self) {
        self.pos += 1;
        self.col += 1;
    }

    fn peek_at(&self, offset: usize) -> Option<u8> {
        self.source.get(self.pos + offset).copied()
    }

    /// The character at the cursor and how many bytes it occupies.
    ///
    /// The lexer walks bytes so that operators and punctuation stay simple, so
    /// multi-byte UTF-8 inside a string literal is decoded explicitly here. An
    /// invalid byte becomes U+FFFD instead of failing the whole token, and is
    /// always consumed, so the loop cannot get stuck.
    fn current_char(&self) -> (char, usize) {
        let rest = &self.source[self.pos..];
        let valid = match std::str::from_utf8(rest) {
            Ok(text) => text.len(),
            Err(error) => error.valid_up_to(),
        };
        if valid == 0 {
            return ('\u{fffd}', 1);
        }
        match std::str::from_utf8(&rest[..valid])
            .expect("valid UTF-8 prefix")
            .chars()
            .next()
        {
            Some(ch) => (ch, ch.len_utf8()),
            None => ('\u{fffd}', 1),
        }
    }

    fn read_token(&mut self) -> Result<TokenKind, LexError> {
        let line = self.line;
        let col = self.col;
        // A comment (or whitespace run) can consume the rest of the input, so
        // the recursion below must be able to stop cleanly at end of input.
        if self.pos >= self.source.len() {
            return Ok(TokenKind::EOF);
        }
        match self.source[self.pos] {
            b'0'..=b'9' => self.read_number(),
            b'a'..=b'z' | b'_' | b'A'..=b'Z' => self.read_identifier_or_keyword(),
            b'"' => self.read_string(),
            b'#' => {
                // A second comment form, for people reaching from Python or
                // shell; `//` works too and both are documented.
                while self.pos < self.source.len() && self.source[self.pos] != b'\n' {
                    self.advance_simple();
                }
                self.read_token()
            }
            b'.' => {
                self.advance_simple();
                Ok(TokenKind::Dot)
            }
            b'+' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::PlusAssign)
                } else {
                    Ok(TokenKind::Plus)
                }
            }
            b'-' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::MinusAssign)
                } else {
                    Ok(TokenKind::Minus)
                }
            }
            b'*' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::StarAssign)
                } else {
                    Ok(TokenKind::Star)
                }
            }
            b'/' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::SlashAssign)
                } else {
                    Ok(TokenKind::Slash)
                }
            }
            b'%' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::PercentAssign)
                } else {
                    Ok(TokenKind::Percent)
                }
            }
            b'(' => {
                self.advance_simple();
                Ok(TokenKind::LeftParen)
            }
            b')' => {
                self.advance_simple();
                Ok(TokenKind::RightParen)
            }
            b'{' => {
                self.advance_simple();
                Ok(TokenKind::LeftBrace)
            }
            b'}' => {
                self.advance_simple();
                Ok(TokenKind::RightBrace)
            }
            b'[' => {
                self.advance_simple();
                Ok(TokenKind::LeftBracket)
            }
            b']' => {
                self.advance_simple();
                Ok(TokenKind::RightBracket)
            }
            b'?' => {
                self.advance_simple();
                Ok(TokenKind::Question)
            }
            b':' => {
                self.advance_simple();
                Ok(TokenKind::Colon)
            }
            b',' => {
                self.advance_simple();
                Ok(TokenKind::Comma)
            }
            b';' => {
                self.advance_simple();
                Ok(TokenKind::Semicolon)
            }
            b'=' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::Equal)
                } else {
                    Ok(TokenKind::Assign)
                }
            }
            b'!' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::NotEqual)
                } else {
                    Ok(TokenKind::Bang)
                }
            }
            b'<' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::LessEqual)
                } else {
                    Ok(TokenKind::Less)
                }
            }
            b'>' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'=') {
                    self.advance_simple();
                    Ok(TokenKind::GreaterEqual)
                } else {
                    Ok(TokenKind::Greater)
                }
            }
            b'&' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'&') {
                    self.advance_simple();
                    Ok(TokenKind::And)
                } else {
                    Err(LexError {
                        message: "unexpected character '&'".to_string(),
                        line,
                        col,
                    })
                }
            }
            b'|' => {
                self.advance_simple();
                if self.peek_at(0) == Some(b'|') {
                    self.advance_simple();
                    Ok(TokenKind::Or)
                } else {
                    Err(LexError {
                        message: "unexpected character '|'".to_string(),
                        line,
                        col,
                    })
                }
            }
            b'\n' => {
                self.advance_simple();
                self.line += 1;
                self.col = 1;
                Ok(TokenKind::Newline)
            }
            _ => Err(LexError {
                message: format!("unexpected character '{}'", self.source[self.pos] as char),
                line,
                col,
            }),
        }
    }

    /// Reads one digit group and the `_`-separated groups that follow it.
    /// Returns false when the character is not a digit.
    fn read_digit_group(&mut self) -> Result<bool, LexError> {
        if self.pos >= self.source.len() || !self.source[self.pos].is_ascii_digit() {
            return Ok(false);
        }
        while self.pos < self.source.len() && self.source[self.pos].is_ascii_digit() {
            self.advance_simple();
        }
        // `1_000_000` and `1.5_5` — underscores separate digits and are not
        // part of the value, like in Python, Rust, and Swift.
        while self.pos < self.source.len() && self.source[self.pos] == b'_' {
            self.advance_simple();
            if self.pos >= self.source.len() || !self.source[self.pos].is_ascii_digit() {
                return Err(LexError {
                    message: "an underscore in a number must be between digits".to_string(),
                    line: self.line,
                    col: self.col,
                });
            }
            while self.pos < self.source.len() && self.source[self.pos].is_ascii_digit() {
                self.advance_simple();
            }
        }
        Ok(true)
    }

    fn read_number(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        let mut is_float = false;
        self.read_digit_group()?;
        if self.pos < self.source.len() && self.source[self.pos] == b'.' {
            is_float = true;
            self.advance_simple();
            if !self.read_digit_group()? {
                return Err(LexError {
                    message: "expected a digit after the decimal point".to_string(),
                    line: self.line,
                    col: self.col,
                });
            }
        }
        // Scientific notation: `1e9`, `2.5e-3`. Nect previously had none, so
        // large constants had to be spelled out.
        if self.pos < self.source.len() && (self.source[self.pos] | 0x20) == b'e' {
            let mut lookahead = self.pos + 1;
            if self.peek_at(lookahead - self.pos) == Some(b'+')
                || self.peek_at(lookahead - self.pos) == Some(b'-')
            {
                lookahead += 1;
            }
            if self
                .source
                .get(lookahead)
                .is_some_and(|b| b.is_ascii_digit())
            {
                is_float = true;
                self.advance_simple(); // e / E
                if self.pos < self.source.len()
                    && (self.source[self.pos] == b'+' || self.source[self.pos] == b'-')
                {
                    self.advance_simple();
                }
                while self.pos < self.source.len() && self.source[self.pos].is_ascii_digit() {
                    self.advance_simple();
                }
            }
            // Anything else starting with `e` belongs to the next token (a
            // variable named `e`, or `else`), so it is left alone.
        }
        let text = std::str::from_utf8(&self.source[start..self.pos])
            .expect("numbers are ASCII")
            .replace('_', "");
        let value: f64 = text.parse().map_err(|_| LexError {
            message: format!("'{}' is not a valid number", text),
            line: self.line,
            col: self.col,
        })?;
        let _ = is_float;
        Ok(TokenKind::Number(value))
    }

    fn read_identifier_or_keyword(&mut self) -> Result<TokenKind, LexError> {
        let start = self.pos;
        while self.pos < self.source.len()
            && (self.source[self.pos].is_ascii_alphanumeric() || self.source[self.pos] == b'_')
        {
            self.advance_simple();
        }
        let s = std::str::from_utf8(&self.source[start..self.pos]).unwrap();
        match s {
            "let" => Ok(TokenKind::Let),
            "fn" => Ok(TokenKind::Func),
            "async" => Ok(TokenKind::Async),
            "extern" => Ok(TokenKind::Extern),
            "await" => Ok(TokenKind::Await),
            "spawn" => Ok(TokenKind::Spawn),
            "if" => Ok(TokenKind::If),
            "else" => Ok(TokenKind::Else),
            "while" => Ok(TokenKind::While),
            "for" => Ok(TokenKind::For),
            "in" => Ok(TokenKind::In),
            "return" => Ok(TokenKind::Return),
            "break" => Ok(TokenKind::Break),
            "continue" => Ok(TokenKind::Continue),
            "true" => Ok(TokenKind::True),
            "false" => Ok(TokenKind::False),
            "null" => Ok(TokenKind::Null),
            // Word spellings of the logic operators, so `if a and b` reads the
            // way people say it. The symbol forms stay equivalent.
            "and" => Ok(TokenKind::AndWord),
            "or" => Ok(TokenKind::OrWord),
            "not" => Ok(TokenKind::NotWord),
            _ => Ok(TokenKind::Identifier(s.to_string())),
        }
    }

    fn read_string(&mut self) -> Result<TokenKind, LexError> {
        let line = self.line;
        let col = self.col;
        self.advance_simple(); // opening "
        let mut result = String::new();
        while self.pos < self.source.len() {
            match self.source[self.pos] {
                b'"' => {
                    self.advance_simple();
                    return Ok(TokenKind::Str(result));
                }
                b'\\' => {
                    self.advance_simple(); // backslash
                    if self.pos >= self.source.len() {
                        break;
                    }
                    match self.source[self.pos] {
                        b'n' => {
                            result.push('\n');
                            self.advance_simple();
                        }
                        b't' => {
                            result.push('\t');
                            self.advance_simple();
                        }
                        b'\\' => {
                            result.push('\\');
                            self.advance_simple();
                        }
                        b'"' => {
                            result.push('"');
                            self.advance_simple();
                        }
                        b'r' => {
                            result.push('\r');
                            self.advance_simple();
                        }
                        b'$' => {
                            // `\$` escapes a literal `${` when interpolation
                            // is not wanted.
                            result.push('$');
                            self.advance_simple();
                        }
                        _ => {
                            let c = self.source[self.pos] as char;
                            result.push(c);
                            self.advance_simple();
                        }
                    }
                }
                b'$' if self.peek_at(1) == Some(b'{') => {
                    // Interpolation: `${expr}` splices an expression's value
                    // into the string. The lexer records the expression's
                    // source between the braces with a marker; the parser
                    // splits the string on the marker and parses each
                    // expression recursively.
                    self.advance_simple(); // $
                    self.advance_simple(); // {
                    let mut depth = 1usize;
                    let expression_start = self.pos;
                    while self.pos < self.source.len() && depth > 0 {
                        match self.source[self.pos] {
                            b'{' => depth += 1,
                            b'}' => depth -= 1,
                            b'\n' => {
                                return Err(LexError {
                                    message: "an interpolated expression cannot span lines"
                                        .to_string(),
                                    line: self.line,
                                    col: self.col,
                                });
                            }
                            _ => {}
                        }
                        self.advance_simple();
                    }
                    if depth > 0 {
                        return Err(LexError {
                            message: "unterminated '${' in a string".to_string(),
                            line: self.line,
                            col: self.col,
                        });
                    }
                    // The closing brace was consumed; the expression is what
                    // sits between the braces.
                    let expression_end = self.pos - 1;
                    let expression =
                        std::str::from_utf8(&self.source[expression_start..expression_end])
                            .expect("the source is UTF-8")
                            .to_string();
                    // Marker layout inside the literal:
                    //   MARK HEAD MARK expression MARK
                    // where HEAD is plain ASCII that cannot collide with user
                    // text. The parser splits on MARK and expects this shape.
                    result.push('\u{1}');
                    result.push_str("NECT-INTERP");
                    result.push('\u{1}');
                    result.push_str(&expression);
                    result.push('\u{1}');
                }
                b'\n' => {
                    result.push('\n');
                    self.advance_simple();
                    self.line += 1;
                    self.col = 1;
                }
                _ => {
                    // Decode UTF-8 so `"héllo"` is five characters, not six bytes.
                    let (ch, width) = self.current_char();
                    result.push(ch);
                    self.pos += width;
                    self.col += 1;
                }
            }
        }
        Err(LexError {
            message: "unterminated string".to_string(),
            line,
            col,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lex(source: &str) -> Vec<TokenKind> {
        Lexer::new(source)
            .tokenize()
            .unwrap()
            .into_iter()
            .map(|t| t.kind)
            .collect()
    }

    #[test]
    // The literal is an arbitrary fraction, not an approximation of pi.
    #[allow(clippy::approx_constant)]
    fn test_numbers() {
        let tokens = lex("42 3.14");
        assert_eq!(tokens[0], TokenKind::Number(42.0));
        assert_eq!(tokens[1], TokenKind::Number(3.14));
        assert_eq!(tokens[2], TokenKind::EOF);
    }

    #[test]
    fn test_string() {
        let tokens = lex("\"hello world\"");
        assert_eq!(tokens[0], TokenKind::Str("hello world".to_string()));
        assert_eq!(tokens[1], TokenKind::EOF);
    }

    #[test]
    fn test_string_escapes() {
        let tokens = lex("\"hello\\nnect\"");
        assert_eq!(tokens[0], TokenKind::Str("hello\nnect".to_string()));
    }

    #[test]
    fn test_identifiers() {
        let tokens = lex("foo bar_baz");
        assert_eq!(tokens[0], TokenKind::Identifier("foo".to_string()));
        assert_eq!(tokens[1], TokenKind::Identifier("bar_baz".to_string()));
    }

    #[test]
    fn test_keywords() {
        let tokens = lex("let fn if else while for in return true false null");
        assert!(matches!(tokens[0], TokenKind::Let));
        assert!(matches!(tokens[1], TokenKind::Func));
        assert!(matches!(tokens[2], TokenKind::If));
        assert!(matches!(tokens[3], TokenKind::Else));
        assert!(matches!(tokens[4], TokenKind::While));
        assert!(matches!(tokens[5], TokenKind::For));
        assert!(matches!(tokens[6], TokenKind::In));
        assert!(matches!(tokens[7], TokenKind::Return));
        assert!(matches!(tokens[8], TokenKind::True));
        assert!(matches!(tokens[9], TokenKind::False));
        assert!(matches!(tokens[10], TokenKind::Null));
    }

    #[test]
    fn test_operators() {
        let tokens = lex("+ - * / % = == != < > <= >= !");
        assert_eq!(tokens[0], TokenKind::Plus);
        assert_eq!(tokens[1], TokenKind::Minus);
        assert_eq!(tokens[2], TokenKind::Star);
        assert_eq!(tokens[3], TokenKind::Slash);
        assert_eq!(tokens[4], TokenKind::Percent);
        assert_eq!(tokens[5], TokenKind::Assign);
        assert_eq!(tokens[6], TokenKind::Equal);
        assert_eq!(tokens[7], TokenKind::NotEqual);
        assert_eq!(tokens[8], TokenKind::Less);
        assert_eq!(tokens[9], TokenKind::Greater);
        assert_eq!(tokens[10], TokenKind::LessEqual);
        assert_eq!(tokens[11], TokenKind::GreaterEqual);
        assert_eq!(tokens[12], TokenKind::Bang);
    }

    #[test]
    fn test_punctuation() {
        let tokens = lex("(){}[],:;");
        assert_eq!(tokens[0], TokenKind::LeftParen);
        assert_eq!(tokens[1], TokenKind::RightParen);
        assert_eq!(tokens[2], TokenKind::LeftBrace);
        assert_eq!(tokens[3], TokenKind::RightBrace);
        assert_eq!(tokens[4], TokenKind::LeftBracket);
        assert_eq!(tokens[5], TokenKind::RightBracket);
        assert_eq!(tokens[6], TokenKind::Comma);
        assert_eq!(tokens[7], TokenKind::Colon);
        assert_eq!(tokens[8], TokenKind::Semicolon);
    }

    #[test]
    fn test_line_comments() {
        let tokens = lex("// this is a comment\n42");
        assert_eq!(tokens[0], TokenKind::Newline);
        assert_eq!(tokens[1], TokenKind::Number(42.0));
    }

    #[test]
    fn test_block_comments() {
        let tokens = lex("/* comment */ 42");
        assert_eq!(tokens[0], TokenKind::Number(42.0));
    }

    #[test]
    fn test_newline_emitted() {
        let tokens = lex("1\n2");
        assert_eq!(tokens[0], TokenKind::Number(1.0));
        assert_eq!(tokens[1], TokenKind::Newline);
        assert_eq!(tokens[2], TokenKind::Number(2.0));
    }

    #[test]
    fn test_logical_operators() {
        let tokens = lex("&& ||");
        assert_eq!(tokens[0], TokenKind::And);
        assert_eq!(tokens[1], TokenKind::Or);
        assert_eq!(tokens[2], TokenKind::EOF);
    }

    #[test]
    fn test_string_decodes_utf8() {
        // The lexer walks bytes; multi-byte characters inside a string must be
        // decoded rather than taken one byte at a time.
        let tokens = lex("\"héllo 🎯\"");
        assert_eq!(tokens[0], TokenKind::Str("héllo 🎯".to_string()));
    }

    #[test]
    fn test_string_with_invalid_utf8_becomes_replacement() {
        let source = format!("\"a{}b\"", '\u{fffd}');
        let tokens = lex(&source);
        assert_eq!(tokens[0], TokenKind::Str("a\u{fffd}b".to_string()));
    }

    #[test]
    fn test_break_and_continue_keywords() {
        let tokens = lex("break continue");
        assert_eq!(tokens[0], TokenKind::Break);
        assert_eq!(tokens[1], TokenKind::Continue);
    }

    #[test]
    fn test_compound_assignment_operators() {
        let tokens = lex("+= -= *= /= %=");
        assert_eq!(tokens[0], TokenKind::PlusAssign);
        assert_eq!(tokens[1], TokenKind::MinusAssign);
        assert_eq!(tokens[2], TokenKind::StarAssign);
        assert_eq!(tokens[3], TokenKind::SlashAssign);
        assert_eq!(tokens[4], TokenKind::PercentAssign);
        assert_eq!(tokens[5], TokenKind::EOF);
    }

    #[test]
    fn test_compound_assignment_is_a_single_token() {
        // `x += 1` must not lex as `+` followed by `=`.
        let tokens = lex("x += 1");
        assert_eq!(tokens[0], TokenKind::Identifier("x".to_string()));
        assert_eq!(tokens[1], TokenKind::PlusAssign);
        assert_eq!(tokens[2], TokenKind::Number(1.0));
    }

    #[test]
    fn test_question_mark() {
        let tokens = lex("a ? b : c");
        assert_eq!(tokens[1], TokenKind::Question);
        assert_eq!(tokens[3], TokenKind::Colon);
    }

    #[test]
    fn test_unexpected_ampersand() {
        let result = Lexer::new("&").tokenize();
        assert!(result.is_err());
    }

    #[test]
    fn test_unterminated_string() {
        let result = Lexer::new("\"unterminated").tokenize();
        assert!(result.is_err());
    }

    #[test]
    fn test_unexpected_character() {
        let result = Lexer::new("@").tokenize();
        assert!(result.is_err());
    }

    #[test]
    fn test_hash_comments() {
        let tokens = lex("# a hash comment\n42");
        assert_eq!(tokens[0], TokenKind::Newline);
        assert_eq!(tokens[1], TokenKind::Number(42.0));
    }

    #[test]
    fn test_digit_separators() {
        let tokens = lex("1_000_000 1.5_5");
        assert_eq!(tokens[0], TokenKind::Number(1_000_000.0));
        assert_eq!(tokens[1], TokenKind::Number(1.55));
    }
    #[test]
    fn test_exponent_literals() {
        let tokens = lex("1.5e2 1e-3 2E+4");
        assert_eq!(tokens[0], TokenKind::Number(150.0));
        assert_eq!(tokens[1], TokenKind::Number(0.001));
        assert_eq!(tokens[2], TokenKind::Number(20000.0));
    }

    #[test]
    fn test_and_or_not_keywords() {
        let tokens = lex("and or not");
        assert!(matches!(tokens[0], TokenKind::AndWord));
        assert!(matches!(tokens[1], TokenKind::OrWord));
        assert!(matches!(tokens[2], TokenKind::NotWord));
    }

    #[test]
    fn test_and_or_not_are_ordinary_identifiers_as_substrings() {
        // Only whole words are keywords; `android` is still a name.
        let tokens = lex("android orchard");
        assert!(matches!(&tokens[0], TokenKind::Identifier(s) if s == "android"));
        assert!(matches!(&tokens[1], TokenKind::Identifier(s) if s == "orchard"));
    }

    #[test]
    fn test_interpolation_string_is_split() {
        // `"a${b}c"` lexes as ONE Str token carrying the expression between
        // markers; the parser splits on the markers and stitches concat().
        let tokens = lex("\"a${b}c\"");
        assert_eq!(
            tokens[0],
            TokenKind::Str("a\u{1}NECT-INTERP\u{1}b\u{1}c".to_string())
        );
        assert_eq!(tokens[1], TokenKind::EOF);
    }

    #[test]
    fn test_plain_string_is_not_split() {
        // A string without `${` stays a single token; a `${` without a closing
        // brace is an error rather than literal text.
        let tokens = lex("\"plain text\"");
        assert_eq!(tokens[0], TokenKind::Str("plain text".to_string()));
        assert!(Lexer::new("\"no ${closure").tokenize().is_err());
    }

    #[test]
    fn test_dollar_outside_string_is_an_error() {
        assert!(Lexer::new("$").tokenize().is_err());
    }
}
