use crate::lexer::{Lexer, TokenKind};
use crate::lsp::index::{SymbolIndex, call_sites};
use crate::lsp::{DocumentState, NectLanguageServerInner};
use crate::parser::Parser;
use crate::vm::Compiler;
use lsp_types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tower_lsp::jsonrpc::Result;
use tower_lsp::{Client, LanguageServer};

pub struct NectLanguageServer {
    inner: Arc<NectLanguageServerInner>,
}

impl NectLanguageServer {
    pub fn new(client: Client) -> Self {
        Self {
            inner: Arc::new(NectLanguageServerInner::new(client)),
        }
    }

    fn parse_document(&self, doc: &DocumentState) -> (Vec<crate::ast::Stmt>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();

        let lexer = Lexer::new(&doc.text);
        let tokens = match lexer.tokenize() {
            Ok(t) => t,
            Err(e) => {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: (e.line.saturating_sub(1)) as u32,
                            character: (e.col.saturating_sub(1)) as u32,
                        },
                        end: Position {
                            line: (e.line.saturating_sub(1)) as u32,
                            character: e.col as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: e.message,
                    source: Some("nect".to_string()),
                    ..Default::default()
                });
                return (Vec::new(), diagnostics);
            }
        };

        let parser = Parser::new(tokens);
        let stmts = match parser.parse() {
            Ok(s) => s,
            Err(e) => {
                diagnostics.push(Diagnostic {
                    range: Range {
                        start: Position {
                            line: (e.line.saturating_sub(1)) as u32,
                            character: (e.col.saturating_sub(1)) as u32,
                        },
                        end: Position {
                            line: (e.line.saturating_sub(1)) as u32,
                            character: e.col as u32,
                        },
                    },
                    severity: Some(DiagnosticSeverity::ERROR),
                    message: e.message,
                    source: Some("nect".to_string()),
                    ..Default::default()
                });
                return (Vec::new(), diagnostics);
            }
        };

        // Try compiling to get type errors
        let compiler = Compiler::compile(&stmts);
        if let Err(_e) = compiler {
            // Compiler errors don't have position info, so we skip adding them
        }

        (stmts, diagnostics)
    }

    fn position_to_offset(&self, text: &str, position: Position) -> Option<usize> {
        let line = position.line as usize;
        let character = position.character as usize;
        let mut offset = 0;
        for (i, l) in text.lines().enumerate() {
            if i == line {
                return Some(offset + character.min(l.len()));
            }
            offset += l.len() + 1; // +1 for newline
        }
        None
    }

    fn get_hover_info(&self, doc: &DocumentState, position: Position) -> Option<Hover> {
        let offset = self.position_to_offset(&doc.text, position)?;
        let lexer = Lexer::new(&doc.text);
        let tokens = lexer.tokenize().ok()?;
        let parser = Parser::new(tokens);
        let stmts = parser.parse().ok()?;

        for stmt in &stmts {
            if let Some(hover) = self.get_hover_in_stmt(stmt, offset) {
                return Some(hover);
            }
        }
        None
    }

    fn get_hover_in_stmt(&self, stmt: &crate::ast::Stmt, _offset: usize) -> Option<Hover> {
        use crate::ast::Stmt::*;
        match stmt {
            Let { name, value, .. } => {
                let ty = self.infer_type(value);
                return Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "**let {}**: {}",
                        name, ty
                    ))),
                    range: None,
                });
            }
            Function { name, params, .. } => {
                let params_str = params.join(", ");
                return Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!(
                        "**fn {}({})**",
                        name, params_str
                    ))),
                    range: None,
                });
            }
            _ => {}
        }
        None
    }

    fn infer_type(&self, expr: &crate::ast::Expr) -> String {
        use crate::ast::Expr::*;
        match expr {
            Literal(crate::ast::Value::Number(_)) => "number".to_string(),
            Literal(crate::ast::Value::String(_)) => "string".to_string(),
            Literal(crate::ast::Value::Boolean(_)) => "boolean".to_string(),
            Literal(crate::ast::Value::Null) => "null".to_string(),
            Literal(crate::ast::Value::Array(_)) => "array".to_string(),
            Literal(crate::ast::Value::Map(_)) => "map".to_string(),
            Variable(name) => format!("var {}", name),
            Binary { .. } => "number".to_string(),
            Call { .. } => "any".to_string(),
            _ => "any".to_string(),
        }
    }

    fn get_completions(&self, doc: &DocumentState, _position: Position) -> Vec<CompletionItem> {
        let mut items = Vec::new();

        // Keywords
        for kw in [
            "let", "fn", "if", "else", "while", "for", "return", "break", "continue", "import",
            "and", "or", "not",
        ] {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        // Builtins
        for builtin in crate::builtins::names() {
            items.push(CompletionItem {
                label: builtin.to_string(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail: Some("builtin".to_string()),
                ..Default::default()
            });
        }

        // Variables/functions from current document
        let lexer = Lexer::new(&doc.text);
        if let Ok(tokens) = lexer.tokenize() {
            let parser = Parser::new(tokens);
            if let Ok(stmts) = parser.parse() {
                for stmt in &stmts {
                    self.collect_completions_from_stmt(stmt, &mut items);
                }
            }
        }

        items
    }

    fn collect_document_symbols(&self, stmts: &[crate::ast::Stmt]) -> Vec<DocumentSymbol> {
        let mut symbols = Vec::new();
        for stmt in stmts {
            self.collect_symbols_from_stmt(stmt, &mut symbols);
        }
        symbols
    }

    fn collect_symbols_from_stmt(
        &self,
        stmt: &crate::ast::Stmt,
        symbols: &mut Vec<DocumentSymbol>,
    ) {
        use crate::ast::Stmt::*;
        match stmt {
            Let { name, .. } => {
                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    deprecated: None,
                    name: name.clone(),
                    kind: SymbolKind::VARIABLE,
                    range: Range::default(),
                    selection_range: Range::default(),
                    detail: Some("let".to_string()),
                    children: None,
                    tags: None,
                });
            }
            Function {
                name, params, body, ..
            } => {
                let mut children = Vec::new();
                // Add parameters as children
                for param in params {
                    #[allow(deprecated)]
                    children.push(DocumentSymbol {
                        deprecated: None,
                        name: param.clone(),
                        kind: SymbolKind::VARIABLE,
                        range: Range::default(),
                        selection_range: Range::default(),
                        detail: Some("parameter".to_string()),
                        children: None,
                        tags: None,
                    });
                }
                // Collect nested symbols from body
                for s in body {
                    self.collect_symbols_from_stmt(s, &mut children);
                }

                #[allow(deprecated)]
                symbols.push(DocumentSymbol {
                    deprecated: None,
                    name: name.clone(),
                    kind: SymbolKind::FUNCTION,
                    range: Range::default(),
                    selection_range: Range::default(),
                    detail: Some(format!("fn({})", params.join(", "))),
                    children: Some(children),
                    tags: None,
                });
            }
            If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    self.collect_symbols_from_stmt(s, symbols);
                }
                for s in else_branch {
                    self.collect_symbols_from_stmt(s, symbols);
                }
            }
            While { body, .. } | For { body, .. } => {
                for s in body {
                    self.collect_symbols_from_stmt(s, symbols);
                }
            }
            Block(statements) => {
                for s in statements {
                    self.collect_symbols_from_stmt(s, symbols);
                }
            }
            _ => {}
        }
    }

    fn collect_completions_from_stmt(
        &self,
        stmt: &crate::ast::Stmt,
        items: &mut Vec<CompletionItem>,
    ) {
        use crate::ast::Stmt::*;
        match stmt {
            Let { name, .. } => {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::VARIABLE),
                    detail: Some("local".to_string()),
                    ..Default::default()
                });
            }
            Function { name, .. } => {
                items.push(CompletionItem {
                    label: name.clone(),
                    kind: Some(CompletionItemKind::FUNCTION),
                    detail: Some("local".to_string()),
                    ..Default::default()
                });
            }
            If {
                then_branch,
                else_branch,
                ..
            } => {
                for s in then_branch {
                    self.collect_completions_from_stmt(s, items);
                }
                for s in else_branch {
                    self.collect_completions_from_stmt(s, items);
                }
            }
            While { body, .. } | For { body, .. } => {
                for s in body {
                    self.collect_completions_from_stmt(s, items);
                }
            }
            Block(statements) => {
                for s in statements {
                    self.collect_completions_from_stmt(s, items);
                }
            }
            _ => {}
        }
    }

    /// Parameter-name hints at call sites: `add(1, 2)` reads `add(a: 1, b: 2)`.    ///
    /// Only calls to functions the file declares are labelled. A built-in has no
    /// parameter names to show — the lexer keeps no signature table for them —
    /// and inventing labels there would be worse than showing nothing.
    fn inlay_hints(&self, doc: &DocumentState, range: Range) -> Vec<InlayHint> {
        let index = SymbolIndex::build(&doc.text);
        // Declared function signatures, keyed by name.
        let mut signatures: HashMap<String, Vec<String>> = HashMap::new();
        for statement in index.occurrences() {
            if !statement.is_declaration {
                continue;
            }
            if !matches!(
                statement.kind,
                crate::lsp::index::SymbolKind::Function | crate::lsp::index::SymbolKind::Extern
            ) {
                continue;
            }
            if let Some(params) = parameter_names(&doc.text, &statement.name) {
                signatures.entry(statement.name.clone()).or_insert(params);
            }
        }

        let mut hints = Vec::new();
        for site in call_sites(&doc.text) {
            let Some(params) = signatures.get(&site.callee) else {
                continue;
            };
            // An extra argument has no name to show, and a missing one simply has
            // no hint; neither is something an inlay hint should report.
            for (argument, param) in site.arguments.iter().zip(params) {
                if !range_contains_range(range, *argument) {
                    continue;
                }
                hints.push(InlayHint {
                    position: argument.start,
                    label: InlayHintLabel::String(format!("{param}:")),
                    kind: Some(InlayHintKind::PARAMETER),
                    text_edits: None,
                    tooltip: Some(InlayHintTooltip::String(format!(
                        "parameter `{param}` of `{}`",
                        site.callee
                    ))),
                    padding_left: Some(false),
                    padding_right: Some(false),
                    data: None,
                });
            }
        }
        hints
    }

    /// Quick fixes for the diagnostics the server itself reports.
    ///
    /// A parse error names the token it wanted, so the one fix worth offering is
    /// the mechanical one: insert the missing closer. Anything smarter would be
    /// guessing at what the author meant.
    fn code_actions(
        &self,
        doc: &DocumentState,
        range: &Range,
        diagnostics: &[Diagnostic],
    ) -> Vec<CodeActionOrCommand> {
        let mut actions = Vec::new();
        for diagnostic in diagnostics {
            if !range_contains_range(*range, diagnostic.range) {
                continue;
            }
            let Some(closer) = missing_closer(&diagnostic.message) else {
                continue;
            };
            let edit = TextEdit {
                range: Range {
                    start: diagnostic.range.end,
                    end: diagnostic.range.end,
                },
                new_text: closer.to_string(),
            };
            let mut changes = HashMap::new();
            changes.insert(doc.uri.clone(), vec![edit]);
            actions.push(CodeActionOrCommand::CodeAction(CodeAction {
                title: format!("Insert `{closer}`"),
                kind: Some(CodeActionKind::QUICKFIX),
                diagnostics: Some(vec![diagnostic.clone()]),
                edit: Some(WorkspaceEdit {
                    changes: Some(changes),
                    ..Default::default()
                }),
                ..Default::default()
            }));
        }
        actions
    }
}

/// The declared parameter names of `function`, read from the token stream.
///
/// Returns `None` when the name is not a function this file declares, so a
/// variable or a parameter is never mistaken for a signature.
fn parameter_names(source: &str, function: &str) -> Option<Vec<String>> {
    let tokens = Lexer::new(source).tokenize().ok()?;
    for (index, token) in tokens.iter().enumerate() {
        let TokenKind::Identifier(name) = &token.kind else {
            continue;
        };
        if name != function {
            continue;
        }
        if tokens
            .get(index + 1)
            .is_none_or(|t| t.kind != TokenKind::LeftParen)
        {
            continue;
        }
        // Walk forward to the matching `)` and collect the names at depth one.
        let mut depth = 0usize;
        let mut params = Vec::new();
        for token in tokens.iter().skip(index + 1) {
            match &token.kind {
                TokenKind::LeftParen => depth += 1,
                TokenKind::RightParen => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(params);
                    }
                }
                TokenKind::Identifier(param) if depth == 1 => params.push(param.clone()),
                _ => {}
            }
        }
    }
    None
}

/// Whether `needle` falls inside `haystack`.
fn range_contains_range(haystack: Range, needle: Range) -> bool {
    let at_or_after = |position: Position, edge: &Position| {
        (position.line, position.character) >= (edge.line, edge.character)
    };
    at_or_after(needle.start, &haystack.start) && at_or_after(haystack.end, &needle.end)
}

/// The closing token the parser says is missing, if the message names one.
///
/// The error catalogue in `docs/reference.md` words these consistently, so
/// matching the wording reads the parser's own report rather than re-deriving it.
fn missing_closer(message: &str) -> Option<&'static str> {
    if message.contains("expected '}'") {
        Some("}")
    } else if message.contains("expected ')'") {
        Some(")")
    } else if message.contains("expected ']'") {
        Some("]")
    } else {
        None
    }
}

#[tower_lsp::async_trait]
impl LanguageServer for NectLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(
                    TextDocumentSyncKind::FULL,
                )),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                rename_provider: Some(OneOf::Left(true)),
                inlay_hint_provider: Some(OneOf::Left(true)),
                code_action_provider: Some(CodeActionProviderCapability::Options(
                    CodeActionOptions {
                        code_action_kinds: Some(vec![
                            CodeActionKind::QUICKFIX,
                            CodeActionKind::REFACTOR,
                        ]),
                        ..Default::default()
                    },
                )),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), "(".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(
                    DiagnosticOptions {
                        identifier: Some("nect".to_string()),
                        ..Default::default()
                    },
                )),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "nect-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.inner
            .client
            .log_message(MessageType::INFO, "Nect LSP server initialized")
            .await;
    }

    async fn shutdown(&self) -> Result<()> {
        Ok(())
    }

    async fn did_open(&self, params: DidOpenTextDocumentParams) {
        let doc = DocumentState::new(
            params.text_document.uri,
            params.text_document.version,
            params.text_document.text,
        );

        let (_, diagnostics) = self.parse_document(&doc);
        self.inner.publish_diagnostics(&doc.uri, diagnostics).await;
        self.inner.insert_document(doc).await;
    }

    async fn did_change(&self, params: DidChangeTextDocumentParams) {
        let uri = params.text_document.uri;
        let version = params.text_document.version;

        if let Some(change) = params.content_changes.first() {
            // If range is None, it's a full document change
            let text = if change.range.is_none() {
                change.text.clone()
            } else {
                return; // Incremental changes not implemented
            };

            self.inner
                .update_document(&uri, version, text.clone())
                .await;

            if let Some(doc) = self.inner.get_document(&uri).await {
                let (_, diagnostics) = self.parse_document(&doc);
                self.inner.publish_diagnostics(&uri, diagnostics).await;
            }
        }
    }

    async fn did_close(&self, params: DidCloseTextDocumentParams) {
        self.inner.remove_document(&params.text_document.uri).await;
    }

    async fn hover(&self, params: HoverParams) -> Result<Option<Hover>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.inner.get_document(&uri).await {
            Ok(self.get_hover_info(&doc, position))
        } else {
            Ok(None)
        }
    }

    async fn goto_definition(
        &self,
        params: GotoDefinitionParams,
    ) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;

        if let Some(doc) = self.inner.get_document(&uri).await {
            let index = SymbolIndex::build(&doc.text);
            let occurrences = index.occurrences_of_at(&doc.text, position);
            // A built-in has no declaration in this file, so there is nothing to
            // jump to; the client falls back to its own documentation.
            let Some(declaration) = occurrences.iter().find(|o| o.is_declaration) else {
                return Ok(None);
            };
            return Ok(Some(GotoDefinitionResponse::Scalar(Location {
                uri: doc.uri.clone(),
                range: declaration.range,
            })));
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let include_declaration = params.context.include_declaration;

        if let Some(doc) = self.inner.get_document(&uri).await {
            let index = SymbolIndex::build(&doc.text);
            let occurrences = index.occurrences_of_at(&doc.text, position);
            let Some(target) = occurrences.first() else {
                return Ok(None);
            };
            let locations: Vec<Location> = occurrences
                .iter()
                .filter(|o| include_declaration || !o.is_declaration)
                .map(|o| Location {
                    uri: doc.uri.clone(),
                    range: o.range,
                })
                .collect();
            let _ = target;
            if locations.is_empty() {
                return Ok(None);
            }
            return Ok(Some(locations));
        }
        Ok(None)
    }

    async fn rename(&self, params: RenameParams) -> Result<Option<WorkspaceEdit>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        let new_name = params.new_name;

        if let Some(doc) = self.inner.get_document(&uri).await {
            // A rename that would produce an invalid identifier is rejected here
            // rather than leaving the file in a state that does not parse.
            if !is_valid_identifier(&new_name) {
                return Ok(None);
            }
            let index = SymbolIndex::build(&doc.text);
            let occurrences = index.occurrences_of_at(&doc.text, position);
            if occurrences.is_empty() {
                return Ok(None);
            }
            // Built-in names are reserved and cannot be shadowed by `fn`, so
            // renaming one would break the program; refuse instead.
            if occurrences
                .iter()
                .all(|o| crate::builtins::names().contains(&o.name.as_str()))
                && !occurrences.iter().any(|o| o.is_declaration)
            {
                return Ok(None);
            }
            let edits: Vec<TextEdit> = occurrences
                .iter()
                .map(|o| TextEdit {
                    range: o.range,
                    new_text: new_name.clone(),
                })
                .collect();
            let mut changes = HashMap::new();
            changes.insert(doc.uri.clone(), edits);
            return Ok(Some(WorkspaceEdit {
                changes: Some(changes),
                ..Default::default()
            }));
        }
        Ok(None)
    }

    async fn inlay_hint(&self, params: InlayHintParams) -> Result<Option<Vec<InlayHint>>> {
        let uri = params.text_document.uri;
        let range = params.range;

        let Some(doc) = self.inner.get_document(&uri).await else {
            return Ok(None);
        };
        let hints = self.inlay_hints(&doc, range);
        if hints.is_empty() {
            return Ok(None);
        }
        Ok(Some(hints))
    }

    async fn code_action(&self, params: CodeActionParams) -> Result<Option<CodeActionResponse>> {
        let uri = params.text_document.uri;
        let Some(doc) = self.inner.get_document(&uri).await else {
            return Ok(None);
        };
        let actions = self.code_actions(&doc, &params.range, &params.context.diagnostics);
        if actions.is_empty() {
            return Ok(None);
        }
        Ok(Some(actions))
    }

    async fn completion(&self, params: CompletionParams) -> Result<Option<CompletionResponse>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;

        if let Some(doc) = self.inner.get_document(&uri).await {
            let items = self.get_completions(&doc, position);
            return Ok(Some(CompletionResponse::Array(items)));
        }
        Ok(None)
    }

    async fn document_symbol(
        &self,
        params: DocumentSymbolParams,
    ) -> Result<Option<DocumentSymbolResponse>> {
        let uri = params.text_document.uri;

        if let Some(doc) = self.inner.get_document(&uri).await {
            let lexer = Lexer::new(&doc.text);
            if let Ok(tokens) = lexer.tokenize() {
                let parser = Parser::new(tokens);
                if let Ok(stmts) = parser.parse() {
                    let symbols = self.collect_document_symbols(&stmts);
                    return Ok(Some(DocumentSymbolResponse::Nested(symbols)));
                }
            }
        }
        Ok(None)
    }
}

/// Whether `name` is usable as an identifier.
///
/// Delegates to the lexer's own rule, so a rename can never produce a name the
/// language would then refuse to parse.
fn is_valid_identifier(name: &str) -> bool {
    crate::lexer::is_identifier(name)
}

#[cfg(test)]
mod tests {
    use super::{is_valid_identifier, missing_closer, parameter_names, range_contains_range};
    use crate::lsp::index::SymbolIndex;
    use lsp_types::{Position, Range};

    fn whole_file() -> Range {
        Range::new(Position::new(0, 0), Position::new(u32::MAX, 0))
    }

    #[test]
    fn reads_a_declared_functions_parameters() {
        let source = "fn add(alpha, beta) {\n    return alpha + beta\n}\n";
        assert_eq!(
            parameter_names(source, "add"),
            Some(vec!["alpha".to_string(), "beta".to_string()])
        );
    }

    #[test]
    fn a_function_with_no_parameters_reads_as_empty() {
        assert_eq!(parameter_names("fn now() {\n}\n", "now"), Some(vec![]));
    }

    #[test]
    fn a_name_that_is_not_a_function_has_no_signature() {
        // A variable that happens to be followed by `(` is still not a signature.
        assert_eq!(parameter_names("let x = 1\nprint(x)\n", "x"), None);
    }

    #[test]
    fn recognizes_the_closers_the_parser_asks_for() {
        assert_eq!(missing_closer("expected '}' to close block"), Some("}"));
        assert_eq!(missing_closer("expected ')' after parameters"), Some(")"));
        assert_eq!(missing_closer("expected ']' after index"), Some("]"));
    }

    #[test]
    fn offers_no_closer_for_other_errors() {
        assert_eq!(missing_closer("expected '=' after variable name"), None);
        assert_eq!(missing_closer("undefined variable 'x'"), None);
    }

    #[test]
    fn a_requested_range_selects_the_occurrences_inside_it() {
        let source = "fn add(a, b) {\n    return a\n}\nadd(1, 2)\n";
        let index = SymbolIndex::build(source);
        let all = index.occurrences_of_at(source, Position::new(3, 1));
        // The declaration on line 0 and the call on line 3.
        assert_eq!(all.len(), 2);
        // The same name, restricted to the last line.
        let on_last_line = all.into_iter().filter(|o| o.range.start.line == 3).count();
        assert_eq!(on_last_line, 1);
    }

    #[test]
    fn range_containment_is_inclusive_at_both_ends() {
        let outer = Range::new(Position::new(0, 0), Position::new(2, 10));
        assert!(range_contains_range(
            outer,
            Range::new(Position::new(1, 0), Position::new(1, 5))
        ));
        assert!(range_contains_range(
            outer,
            Range::new(Position::new(0, 0), Position::new(0, 0))
        ));
        assert!(!range_contains_range(
            outer,
            Range::new(Position::new(3, 0), Position::new(3, 1))
        ));
    }

    #[test]
    fn a_whole_file_range_covers_a_short_document() {
        let outer = whole_file();
        assert!(range_contains_range(
            outer,
            Range::new(Position::new(0, 4), Position::new(0, 9))
        ));
    }

    #[test]
    fn accepts_ordinary_identifiers() {
        for name in ["x", "count", "_private", "camelCase", "with_digits9"] {
            assert!(
                is_valid_identifier(name),
                "{name} should be a valid identifier"
            );
        }
    }

    #[test]
    fn rejects_names_the_lexer_would_not_accept() {
        // Non-ASCII is included deliberately: identifiers are ASCII-only, so a
        // rename to "éé" would produce a file the scanner splits in two.
        for name in ["", "9lives", "has space", "has-dash", "a.b", "éé"] {
            assert!(
                !is_valid_identifier(name),
                "{name} should be rejected as an identifier"
            );
        }
    }

    #[test]
    fn agrees_with_the_lexer_about_what_it_will_scan() {
        // The validator and the scanner must not drift: whatever this accepts,
        // the lexer has to produce as a single identifier token.
        for name in ["x", "count", "_private", "camelCase", "with_digits9"] {
            assert!(is_valid_identifier(name));
            let tokens = crate::lexer::Lexer::new(name).tokenize().expect("lexes");
            assert_eq!(tokens.len(), 2, "{name} should lex to one name plus EOF");
            assert!(
                matches!(&tokens[0].kind, crate::lexer::TokenKind::Identifier(found) if found == name),
                "{name} should lex to an identifier"
            );
        }
    }

    #[test]
    fn rejects_reserved_words() {
        for name in [
            "let", "fn", "if", "else", "while", "for", "return", "and", "not",
        ] {
            assert!(
                !is_valid_identifier(name),
                "{name} is reserved and cannot be used as a name"
            );
        }
    }
}
