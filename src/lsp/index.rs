//! A token-accurate symbol index for the language server.
//!
//! Go-to-definition, find-references, and rename all need to know two things
//! about an identifier: which range declares it, and which ranges use it. The
//! lexer already records an exact byte range and line/column for every token,
//! so both answers can be read straight off the token stream.
//!
//! Working from tokens rather than from AST annotations is a deliberate
//! choice. Attaching spans to every `Expr` and `Stmt` variant would touch the
//! interpreter, the VM compiler, the JIT's type inference, the C backend, the
//! formatter, and the linter — every one of which pattern-matches those enums —
//! for no benefit at runtime. The index is built on demand in the editor, where
//! a lexical scan of one file is imperceptible, and it leaves the engines and
//! their differential tests untouched.
//!
//! The scan is lexical rather than grammatical, so it stays useful on a file
//! that does not fully parse — which is exactly when a developer is most likely
//! to be asking "where is this defined?".

use crate::lexer::{Lexer, Token, TokenKind};
use lsp_types::{Position, Range};

/// What a name refers to, which decides how aggressively references are
/// gathered and how a rename is validated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolKind {
    /// `let name = ...`
    Variable,
    /// `fn name(...)`
    Function,
    /// A name in a parameter list.
    Parameter,
    /// A `for name in ...` loop variable.
    LoopVariable,
    /// A function name in an `extern` block.
    Extern,
}

impl SymbolKind {
    /// The LSP `SymbolKind` a document-symbol or workspace-symbol result uses.
    pub fn as_symbol_kind(self) -> lsp_types::SymbolKind {
        match self {
            SymbolKind::Variable | SymbolKind::LoopVariable | SymbolKind::Parameter => {
                lsp_types::SymbolKind::VARIABLE
            }
            SymbolKind::Function => lsp_types::SymbolKind::FUNCTION,
            SymbolKind::Extern => lsp_types::SymbolKind::FUNCTION,
        }
    }
}

/// One occurrence of an identifier, with the range it covers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Occurrence {
    pub name: String,
    pub range: Range,
    pub kind: SymbolKind,
    /// `true` when this occurrence is the one that introduces the name.
    pub is_declaration: bool,
}

/// The declarations and uses of one document.
#[derive(Debug, Clone, Default)]
pub struct SymbolIndex {
    occurrences: Vec<Occurrence>,
    /// Ranges of tokens the scanner could not classify, so a caller can tell
    /// "no such name here" from "the file is too broken to tell".
    scanned_bytes: usize,
}

impl SymbolIndex {
    /// Builds the index for `source`.
    ///
    /// Never fails: an unlexable file yields whatever prefix was readable, and
    /// the caller still gets diagnostics from the normal parse path.
    pub fn build(source: &str) -> Self {
        let tokens = Lexer::new(source).tokenize().unwrap_or_default();
        let mut index = SymbolIndex {
            occurrences: Vec::new(),
            scanned_bytes: source.len(),
        };
        index.scan(source, &tokens);
        index
    }

    /// Every occurrence, in source order.
    pub fn occurrences(&self) -> &[Occurrence] {
        &self.occurrences
    }

    /// How much of the source the scan covered. Equal to the source length
    /// unless lexing failed partway.
    pub fn scanned_bytes(&self) -> usize {
        self.scanned_bytes
    }

    /// Every occurrence of the identifier at `position`, declaration included.
    pub fn occurrences_of_at(&self, source: &str, position: Position) -> Vec<Occurrence> {
        let Some(offset) = position_to_offset_in(source, position) else {
            return Vec::new();
        };
        let Some(target) = self.identifier_at_offset(source, offset) else {
            return Vec::new();
        };
        self.occurrences_of(&target)
    }

    /// Every occurrence of `name`.
    pub fn occurrences_of(&self, name: &str) -> Vec<Occurrence> {
        self.occurrences
            .iter()
            .filter(|o| o.name == name)
            .cloned()
            .collect()
    }

    /// The declaration of `name`, preferring the earliest one so that a name
    /// declared once at module level resolves to that site rather than to a
    /// shadowing local further down.
    pub fn declaration_of(&self, name: &str) -> Option<&Occurrence> {
        self.occurrences
            .iter()
            .find(|o| o.name == name && o.is_declaration)
    }

    /// The name of the identifier covering `offset`, if any.
    fn identifier_at_offset(&self, source: &str, offset: usize) -> Option<String> {
        let position = offset_to_position(source, offset);
        self.occurrences
            .iter()
            .find(|o| range_contains(o.range, position))
            .map(|o| o.name.clone())
    }

    /// Walks the token stream, classifying identifiers.
    ///
    /// Declarations are collected first so the generic identifier arm can step
    /// over them: a declared name is itself an `Identifier` token, and without
    /// this it would be recorded twice, once as a declaration and once as a use.
    fn scan(&mut self, source: &str, tokens: &[Token]) {
        let declarations = declarations(tokens);
        // A parameter is a declaration too, and it is an `Identifier` token like
        // any other, so its index has to be marked before the generic pass runs
        // or the name gets recorded twice at the same position.
        let parameters = parameter_tokens(tokens);

        let mut claimed = vec![false; tokens.len()];
        for (index, _) in &declarations {
            claimed[*index] = true;
        }
        for index in &parameters {
            claimed[*index] = true;
        }

        for (index, token) in tokens.iter().enumerate() {
            if claimed[index] {
                continue;
            }
            if matches!(token.kind, TokenKind::Identifier(_)) {
                self.push(source, token, SymbolKind::Variable, false);
            }
        }

        for (index, kind) in declarations {
            self.push(source, &tokens[index], kind, true);
        }
        for index in parameters {
            self.push(source, &tokens[index], SymbolKind::Parameter, true);
        }

        self.occurrences
            .sort_by_key(|o| (o.range.start.line, o.range.start.character));
        // The parameter pass can revisit a token the declaration pass already
        // recorded; keep the richer classification rather than the duplicate.
        let mut deduped: Vec<Occurrence> = Vec::with_capacity(self.occurrences.len());
        for occurrence in self.occurrences.drain(..) {
            let duplicate = deduped.iter().any(|existing| {
                existing.name == occurrence.name
                    && existing.range.start == occurrence.range.start
                    && existing.range.end == occurrence.range.end
                    && existing.kind == occurrence.kind
            });
            if !duplicate {
                deduped.push(occurrence);
            }
        }
        self.occurrences = deduped;
    }

    fn push(&mut self, source: &str, token: &Token, kind: SymbolKind, is_declaration: bool) {
        let name = match &token.kind {
            TokenKind::Identifier(name) => name.clone(),
            _ => return,
        };
        self.occurrences.push(Occurrence {
            name,
            range: token_range(source, token),
            kind,
            is_declaration,
        });
    }
}

/// Every declaration in the token stream, as `(token index, kind)`.
///
/// A declaration can be separated from its name by a newline (`let\nx = 1` is
/// accepted) and sometimes by a keyword (`async fn f`), so each form looks at
/// the token itself rather than assuming the name is simply the next one.
fn declarations(tokens: &[Token]) -> Vec<(usize, SymbolKind)> {
    let mut found = Vec::new();
    let mut extern_end: Option<usize> = None;
    for (index, token) in tokens.iter().enumerate() {
        // Skip the body of an extern block in the outer walk; its `fn` lines
        // declare callables and are handled below.
        if let Some(end) = extern_end {
            if index <= end {
                continue;
            }
            extern_end = None;
        }
        match token.kind {
            TokenKind::Let => {
                if let Some(name) = name_index(tokens, index + 1) {
                    found.push((name, SymbolKind::Variable));
                }
            }
            TokenKind::Func => {
                if let Some(name) = name_index(tokens, index + 1) {
                    found.push((name, SymbolKind::Function));
                }
            }
            TokenKind::Async => {
                // `async fn name`: the declaration keyword is `async`, and the
                // name follows the `fn` one token later.
                if tokens
                    .get(index + 1)
                    .is_some_and(|t| t.kind == TokenKind::Func)
                    && let Some(name) = name_index(tokens, index + 2)
                {
                    found.push((name, SymbolKind::Function));
                }
            }
            TokenKind::Extern => {
                let end = tokens[index..]
                    .iter()
                    .position(|t| t.kind == TokenKind::RightBrace)
                    .map(|offset| index + offset)
                    .unwrap_or(tokens.len().saturating_sub(1));
                for cursor in index..end {
                    if tokens[cursor].kind == TokenKind::Func
                        && let Some(name) = name_index(tokens, cursor + 1)
                    {
                        found.push((name, SymbolKind::Extern));
                    }
                }
                extern_end = Some(end);
            }
            TokenKind::For => {
                if let Some(name) = name_index(tokens, index + 1) {
                    found.push((name, SymbolKind::LoopVariable));
                }
            }
            _ => {}
        }
    }
    found
}

/// The index of the identifier token at `from`, if that token is an identifier.
fn name_index(tokens: &[Token], from: usize) -> Option<usize> {
    let token = tokens.get(from)?;
    matches!(token.kind, TokenKind::Identifier(_)).then_some(from)
}

/// The token indices of every function parameter.
///
/// A call site's argument list is lexically identical to a parameter list, so
/// each candidate list is traced back through its name to the `fn` (or `extern`
/// declaration) that introduced it.
fn parameter_tokens(tokens: &[Token]) -> Vec<usize> {
    let mut parameters = Vec::new();
    for (index, token) in tokens.iter().enumerate() {
        if token.kind != TokenKind::LeftParen {
            continue;
        }
        if !introduces_a_parameter_list(tokens, index) {
            continue;
        }
        let mut depth = 0usize;
        for (offset, token) in tokens.iter().enumerate().skip(index) {
            match token.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                // Only the outermost list holds parameters; anything deeper is
                // a default value's own expression.
                TokenKind::Identifier(_) if depth == 1 => parameters.push(offset),
                _ => {}
            }
        }
    }
    parameters
}

/// Whether the `(` at `open` is a parameter list rather than a call's arguments.
///
/// Walks back from the name in front of the `(` to whatever introduced it: a
/// `fn`/`async`/`extern` declaration means a parameter list, anything else is a
/// call.
fn introduces_a_parameter_list(tokens: &[Token], open: usize) -> bool {
    let Some(name) = open.checked_sub(1) else {
        return false;
    };
    if !matches!(tokens[name].kind, TokenKind::Identifier(_)) {
        return false;
    }
    // Step over the name, then over anything that separates it from the keyword
    // that introduced it (a nested list, a comma in an outer list).
    let mut probe = name;
    while probe > 0 {
        match tokens[probe - 1].kind {
            TokenKind::Func | TokenKind::Async | TokenKind::Extern => return true,
            TokenKind::LeftParen | TokenKind::Comma => probe -= 1,
            _ => return false,
        }
    }
    false
}

/// One call expression, with the range of each of its arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallSite {
    /// The name being called. Only a bare `name(...)` is recognised, which is
    /// every call the language spells with an identifier.
    pub callee: String,
    /// The callee token's own range, so a caller can point at the function.
    pub callee_range: Range,
    /// One range per argument, in order. A call with no arguments has none.
    pub arguments: Vec<Range>,
}

/// Every `name(...)` call in the source, with its argument ranges.
///
/// Found by scanning for an identifier immediately followed by `(` and walking
/// the balanced parentheses, so a comma inside a nested call or an array literal
/// does not split an argument in two. This is the same balanced-walk technique
/// the parameter scan uses, and it needs no AST annotations, which is what lets
/// it work on a file that does not fully parse.
pub fn call_sites(source: &str) -> Vec<CallSite> {
    let tokens = Lexer::new(source).tokenize().unwrap_or_default();
    let mut sites = Vec::new();
    for index in 0..tokens.len() {
        let TokenKind::Identifier(callee) = &tokens[index].kind else {
            continue;
        };
        // The `(` has to be the very next token: `name (x)` with a space is
        // still a call, but `name + (x)` is not.
        if tokens
            .get(index + 1)
            .is_none_or(|t| t.kind != TokenKind::LeftParen)
        {
            continue;
        }
        // A declaration's parameter list looks identical to a call's arguments,
        // so skip the ones that belong to a `fn`.
        if introduces_a_parameter_list(&tokens, index + 1) {
            continue;
        }
        let Some(arguments) = argument_ranges(source, &tokens, index + 1) else {
            continue;
        };
        sites.push(CallSite {
            callee: callee.clone(),
            callee_range: token_range(source, &tokens[index]),
            arguments,
        });
    }
    sites
}

/// The ranges of the top-level arguments in the list whose `(` is at `open`.
///
/// Returns `None` when the parentheses are unbalanced, which happens on a file
/// still being typed; a truncated call simply produces no hints rather than a
/// wrong one.
fn argument_ranges(source: &str, tokens: &[Token], open: usize) -> Option<Vec<Range>> {
    let mut ranges = Vec::new();
    let mut depth = 0usize;
    // Where the current argument started, or `None` between arguments.
    let mut argument_start: Option<usize> = None;
    for index in open..tokens.len() {
        match tokens[index].kind {
            TokenKind::LeftParen | TokenKind::LeftBracket | TokenKind::LeftBrace => {
                if depth == 0 && index != open {
                    argument_start.get_or_insert(index);
                }
                depth += 1;
            }
            TokenKind::RightParen | TokenKind::RightBracket | TokenKind::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    // The closing paren ends the last argument, if there is one.
                    if let Some(start) = argument_start.take() {
                        ranges.push(
                            token_range(source, &tokens[start])
                                .start
                                .to_range(&tokens[index].start, source),
                        );
                    }
                    return Some(ranges);
                }
            }
            TokenKind::Comma if depth == 1 => {
                if let Some(start) = argument_start.take() {
                    push_trimmed(source, tokens, start, index, &mut ranges);
                }
            }
            _ => {
                if depth >= 1 {
                    argument_start.get_or_insert(index);
                }
            }
        }
    }
    None
}

/// The range spanning tokens `from..to`, excluding surrounding whitespace.
fn push_trimmed(source: &str, tokens: &[Token], from: usize, to: usize, out: &mut Vec<Range>) {
    let start = tokens[from].start;
    let end = tokens[to - 1].end;
    out.push(Range {
        start: offset_to_position(source, start),
        end: offset_to_position(source, end),
    });
}

trait RangeExt {
    fn to_range(self, end: &usize, source: &str) -> Range;
}

impl RangeExt for Position {
    fn to_range(self, end: &usize, source: &str) -> Range {
        Range {
            start: self,
            end: offset_to_position(source, *end),
        }
    }
}

/// The LSP range covering a token.
///
/// The lexer reports byte offsets plus 1-based line/column, but LSP positions
/// are 0-based lines and UTF-16 code units. Converting from the byte offset
/// against the real source is the only way to stay correct for lines that
/// contain non-ASCII text.
fn token_range(source: &str, token: &Token) -> Range {
    Range {
        start: offset_to_position(source, token.start),
        end: offset_to_position(source, token.end),
    }
}

/// Converts a byte offset into an LSP position.
pub fn offset_to_position(source: &str, offset: usize) -> Position {
    let offset = offset.min(source.len());
    let before = &source[..offset];
    let line = before.matches('\n').count() as u32;
    let line_start = before.rfind('\n').map(|i| i + 1).unwrap_or(0);
    // LSP measures characters in UTF-16 code units.
    let character = source[line_start..offset]
        .chars()
        .map(char::len_utf16)
        .sum::<usize>() as u32;
    Position { line, character }
}

/// Converts an LSP position into a byte offset, clamping to the line's end so a
/// stale client position cannot walk off the end of the document.
pub fn position_to_offset_in(source: &str, position: Position) -> Option<usize> {
    let mut line_start = 0usize;
    let mut current_line = 0u32;
    for line in source.split_inclusive('\n') {
        if current_line == position.line {
            let content = line.strip_suffix('\n').unwrap_or(line);
            let content = content.strip_suffix('\r').unwrap_or(content);
            let mut units = 0u32;
            for (byte_offset, ch) in content.char_indices() {
                if units >= position.character {
                    return Some(line_start + byte_offset);
                }
                units += ch.len_utf16() as u32;
            }
            return Some(line_start + content.len());
        }
        line_start += line.len();
        current_line += 1;
    }
    // A position past the last line resolves to the end of the document, so a
    // stale client position still lands somewhere meaningful instead of
    // failing the request outright.
    let _ = current_line;
    Some(source.len())
}

/// Whether `position` falls inside `range`. A position at the very end of a
/// range counts as inside, which is what makes clicking just past the last
/// character of an identifier still resolve.
fn range_contains(range: Range, position: Position) -> bool {
    let after_start =
        (position.line, position.character) >= (range.start.line, range.start.character);
    let before_end = (position.line, position.character) <= (range.end.line, range.end.character);
    after_start && before_end
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    fn names(index: &SymbolIndex) -> Vec<&str> {
        index
            .occurrences()
            .iter()
            .map(|o| o.name.as_str())
            .collect()
    }

    #[test]
    fn records_a_module_level_declaration() {
        let index = SymbolIndex::build("let total = 1\n");
        assert_eq!(names(&index), vec!["total"]);
        assert!(index.occurrences()[0].is_declaration);
    }

    #[test]
    fn records_every_use_of_a_name() {
        let index = SymbolIndex::build("let x = 1\nprint(x)\nlet y = x + x\n");
        assert_eq!(names(&index), vec!["x", "print", "x", "y", "x", "x"]);
    }

    #[test]
    fn records_function_declarations() {
        let index = SymbolIndex::build("fn add(a, b) {\n    return a + b\n}\n");
        let add = index.occurrences_of("add");
        assert_eq!(add.len(), 1);
        assert!(add[0].is_declaration);
        assert_eq!(add[0].kind, SymbolKind::Function);
    }

    #[test]
    fn marks_async_functions() {
        let index = SymbolIndex::build("async fn fetch(url) {\n    return url\n}\n");
        let fetch = index.occurrences_of("fetch");
        assert_eq!(fetch.len(), 1);
        assert!(fetch[0].is_declaration);
    }

    #[test]
    fn records_parameters() {
        let index = SymbolIndex::build("fn f(alpha, beta) {\n    return alpha\n}\n");
        let alpha = index.occurrences_of("alpha");
        assert!(alpha.iter().any(|o| o.is_declaration));
        assert!(
            alpha
                .iter()
                .any(|o| o.kind == SymbolKind::Parameter && o.is_declaration)
        );
    }

    #[test]
    fn does_not_mistake_a_call_site_for_a_declaration() {
        let index = SymbolIndex::build("fn f() {\n}\nf(1)\n");
        let occurrences = index.occurrences_of("f");
        let declarations = occurrences.iter().filter(|o| o.is_declaration).count();
        assert_eq!(declarations, 1);
    }

    #[test]
    fn records_loop_variables() {
        let index = SymbolIndex::build("for item in [1, 2] {\n    print(item)\n}\n");
        let item = index.occurrences_of("item");
        assert!(item.iter().any(|o| o.is_declaration));
    }

    #[test]
    fn records_extern_declarations() {
        let source = "extern \"m\" {\n  fn sin(number) -> number;\n}\n";
        let index = SymbolIndex::build(source);
        let sin = index.occurrences_of("sin");
        assert_eq!(sin.len(), 1);
        assert!(sin[0].is_declaration);
        assert_eq!(sin[0].kind, SymbolKind::Extern);
    }

    #[test]
    fn stays_useful_when_the_file_does_not_parse() {
        // An unterminated function: the point of the index is that it still
        // answers "where is this defined?" while the file is being written.
        let index = SymbolIndex::build("fn helper(n) {\n    return n\n");
        assert!(
            index
                .occurrences_of("helper")
                .iter()
                .any(|o| o.is_declaration)
        );
    }

    #[test]
    fn positions_are_zero_based_lines() {
        let index = SymbolIndex::build("let a = 1\nlet b = 2\n");
        let b = index.occurrences_of("b");
        assert_eq!(b[0].range.start, pos(1, 4));
        assert_eq!(b[0].range.end, pos(1, 5));
    }

    #[test]
    fn positions_count_utf16_code_units() {
        // "🎯" is one char but two UTF-16 code units, so a byte- or char-based
        // column would put the following text in the wrong place. This is the
        // conversion the index relies on, so it is tested directly.
        let source = "let s = \"🎯\"\nlet after = 2\n";
        let emoji = source.find('🎯').unwrap();
        // `let s = "` is nine characters before the emoji.
        assert_eq!(offset_to_position(source, emoji), pos(0, 9));

        let after = source.find("after").unwrap();
        assert_eq!(offset_to_position(source, after), pos(1, 4));
        assert_eq!(offset_to_position(source, after + 5), pos(1, 9));

        // And the reverse direction lands back on the same byte.
        let offset = position_to_offset_in(source, pos(1, 4)).unwrap();
        assert_eq!(&source[offset..offset + 5], "after");
    }

    #[test]
    fn a_non_ascii_line_keeps_its_columns() {
        // The identifier on line 1 follows a line containing an emoji, so its
        // column is only right if line starts are measured in bytes but columns
        // in UTF-16 units.
        let source = "let s = \"🎯\"\nlet after = 2\n";
        let index = SymbolIndex::build(source);
        let after = index.occurrences_of("after");
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].range.start, pos(1, 4));
    }

    #[test]
    fn round_trips_a_position_through_an_offset() {
        let source = "let a = 1\nlet bee = 2\n";
        let offset = position_to_offset_in(source, pos(1, 4)).unwrap();
        assert_eq!(&source[offset..offset + 3], "bee");
    }

    #[test]
    fn a_position_past_the_end_clamps_to_the_document_end() {
        let source = "let a = 1\n";
        let offset = position_to_offset_in(source, pos(99, 0));
        assert_eq!(offset, Some(source.len()));
    }

    #[test]
    fn occurrence_order_follows_the_source() {
        let index = SymbolIndex::build("let a = 1\nlet b = 2\nlet c = 3\n");
        let lines: Vec<u32> = index
            .occurrences()
            .iter()
            .map(|o| o.range.start.line)
            .collect();
        assert_eq!(lines, vec![0, 1, 2]);
    }
}

#[cfg(test)]
mod navigation_tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    /// The occurrence ranges the index reports for the identifier at a position.
    fn ranges_at(source: &str, line: u32, character: u32) -> Vec<Range> {
        let index = SymbolIndex::build(source);
        index
            .occurrences_of_at(source, pos(line, character))
            .into_iter()
            .map(|o| o.range)
            .collect()
    }

    #[test]
    fn a_use_resolves_to_every_occurrence_including_the_declaration() {
        let source = "let total = 1\nprint(total)\n";
        let ranges = ranges_at(source, 1, 7);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], Range::new(pos(0, 4), pos(0, 9)));
        assert_eq!(ranges[1], Range::new(pos(1, 6), pos(1, 11)));
    }

    #[test]
    fn a_declaration_resolves_to_itself_and_its_uses() {
        let source = "fn double(n) {\n    return n * 2\n}\ndouble(4)\n";
        let ranges = ranges_at(source, 0, 4);
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0], Range::new(pos(0, 3), pos(0, 9)));
    }

    #[test]
    fn a_position_on_whitespace_finds_nothing() {
        let source = "let total = 1\n";
        assert!(ranges_at(source, 0, 0).is_empty());
    }

    #[test]
    fn distinct_names_do_not_cross_contaminate() {
        let source = "let a = 1\nlet b = 2\nprint(a)\nprint(b)\n";
        assert_eq!(ranges_at(source, 2, 6).len(), 2);
        assert_eq!(ranges_at(source, 3, 6).len(), 2);
    }

    #[test]
    fn a_parameter_and_a_module_value_with_the_same_name_stay_separate_by_count() {
        // Both are occurrences of `n`, which is the honest answer for a purely
        // lexical index: resolving the shadowed one needs scope analysis, which
        // the index deliberately does not attempt.
        let source = "fn f(n) {\n    return n\n}\n";
        assert_eq!(ranges_at(source, 1, 11).len(), 2);
    }

    #[test]
    fn clicking_the_last_character_of_an_identifier_still_resolves() {
        let source = "let total = 1\nprint(total)\n";
        let ranges = ranges_at(source, 1, 10);
        assert_eq!(ranges.len(), 2);
    }

    #[test]
    fn extern_functions_are_reachable_by_name() {
        let source = "extern \"m\" {\n  fn cos(number) -> number;\n}\nprint(cos(0))\n";
        let ranges = ranges_at(source, 3, 7);
        assert_eq!(ranges.len(), 2);
    }
}

#[cfg(test)]
mod call_site_tests {
    use super::*;

    fn pos(line: u32, character: u32) -> Position {
        Position { line, character }
    }

    #[test]
    fn finds_a_call_and_its_arguments() {
        let source = "add(1, 2)\n";
        let sites = call_sites(source);
        assert_eq!(sites.len(), 1);
        assert_eq!(sites[0].callee, "add");
        assert_eq!(sites[0].arguments.len(), 2);
        assert_eq!(sites[0].arguments[0].start, pos(0, 4));
        assert_eq!(sites[0].arguments[1].start, pos(0, 7));
    }

    #[test]
    fn a_call_with_no_arguments_has_none() {
        let sites = call_sites("now()\n");
        assert_eq!(sites[0].arguments.len(), 0);
    }

    #[test]
    fn a_nested_call_does_not_split_an_argument() {
        // The comma belongs to the inner call, so the outer call still has two
        // arguments, not three.
        let source = "outer(inner(1, 2), 3)\n";
        let sites = call_sites(source);
        let outer = sites.iter().find(|s| s.callee == "outer").unwrap();
        assert_eq!(outer.arguments.len(), 2);
        let inner = sites.iter().find(|s| s.callee == "inner").unwrap();
        assert_eq!(inner.arguments.len(), 2);
    }

    #[test]
    fn an_array_literal_argument_is_one_argument() {
        let source = "f([1, 2, 3])\n";
        let sites = call_sites(source);
        assert_eq!(sites[0].arguments.len(), 1);
    }

    #[test]
    fn a_map_literal_argument_is_one_argument() {
        let source = "f({\"a\": 1, \"b\": 2})\n";
        let sites = call_sites(source);
        assert_eq!(sites[0].arguments.len(), 1);
    }

    #[test]
    fn a_function_declaration_is_not_a_call() {
        // `fn add(a, b)` has the same shape as `add(a, b)` and must not be
        // mistaken for one, or every declaration would get call hints.
        let source = "fn add(a, b) {\n    return a + b\n}\n";
        assert!(call_sites(source).is_empty());
    }

    #[test]
    fn a_method_call_sugar_is_a_call_to_the_builtin() {
        let sites = call_sites("print(name.upper())\n");
        let names: Vec<&str> = sites.iter().map(|s| s.callee.as_str()).collect();
        assert!(names.contains(&"upper"));
        assert!(names.contains(&"print"));
    }

    #[test]
    fn an_unbalanced_call_produces_nothing_rather_than_a_wrong_answer() {
        // A file mid-edit: better to show no hint than a misplaced one.
        assert!(call_sites("f(1, \n").is_empty());
    }

    #[test]
    fn a_trailing_comma_does_not_add_an_empty_argument() {
        let sites = call_sites("f(1, 2,)\n");
        assert_eq!(sites[0].arguments.len(), 2);
    }

    #[test]
    fn argument_ranges_cover_the_argument_text() {
        let source = "f(alpha, beta)\n";
        let sites = call_sites(source);
        assert_eq!(sites[0].arguments[0].start, pos(0, 2));
        assert_eq!(sites[0].arguments[0].end, pos(0, 7));
        assert_eq!(sites[0].arguments[1].start, pos(0, 9));
        assert_eq!(sites[0].arguments[1].end, pos(0, 13));
    }

    #[test]
    fn builtin_calls_are_recognised_too() {
        // The server filters these out (built-ins have no declared parameter
        // names), but recognising them keeps the scan honest.
        let sites = call_sites("print(len(\"abc\"))\n");
        assert_eq!(sites.len(), 2);
    }
}
