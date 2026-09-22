use crate::lsp::{DocumentState, NectLanguageServerInner};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::vm::Compiler;
use lsp_types::*;
use std::sync::Arc;
use tower_lsp::{Client, LanguageServer};
use tower_lsp::jsonrpc::Result;

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
                        start: Position { line: (e.line.saturating_sub(1)) as u32, character: (e.col.saturating_sub(1)) as u32 },
                        end: Position { line: (e.line.saturating_sub(1)) as u32, character: e.col as u32 },
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
                        start: Position { line: (e.line.saturating_sub(1)) as u32, character: (e.col.saturating_sub(1)) as u32 },
                        end: Position { line: (e.line.saturating_sub(1)) as u32, character: e.col as u32 },
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

    fn find_definition(&self, doc: &DocumentState, position: Position) -> Option<Location> {
        let offset = self.position_to_offset(&doc.text, position)?;
        let lexer = Lexer::new(&doc.text);
        let tokens = lexer.tokenize().ok()?;
        let parser = Parser::new(tokens);
        let stmts = parser.parse().ok()?;

        for stmt in &stmts {
            if let Some(loc) = self.find_definition_in_stmt(stmt, offset, &doc.uri) {
                return Some(loc);
            }
        }
        None
    }

    fn find_definition_in_stmt(&self, stmt: &crate::ast::Stmt, offset: usize, uri: &Url) -> Option<Location> {
        use crate::ast::Stmt::*;
        match stmt {
            Let { name, value, .. } => {
                if let Some(loc) = self.find_definition_in_expr(value, offset, uri, name) {
                    return Some(loc);
                }
            }
            Function { name, params, body, .. } => {
                if let Some(loc) = self.find_definition_in_function(name, params, body, offset, uri) {
                    return Some(loc);
                }
            }
            _ => {}
        }
        // Recursively check nested statements
        match stmt {
            If { then_branch, else_branch, .. } => {
                for s in then_branch {
                    if let Some(loc) = self.find_definition_in_stmt(s, offset, uri) {
                        return Some(loc);
                    }
                }
                for s in else_branch {
                    if let Some(loc) = self.find_definition_in_stmt(s, offset, uri) {
                        return Some(loc);
                    }
                }
            }
            While { body, .. } | For { body, .. } => {
                for s in body {
                    if let Some(loc) = self.find_definition_in_stmt(s, offset, uri) {
                        return Some(loc);
                    }
                }
            }
            Block(statements) => {
                for s in statements {
                    if let Some(loc) = self.find_definition_in_stmt(s, offset, uri) {
                        return Some(loc);
                    }
                }
            }
            _ => {}
        }
        None
    }

    fn find_definition_in_expr(&self, expr: &crate::ast::Expr, offset: usize, uri: &Url, name: &str) -> Option<Location> {
        // Simple check - if the offset falls within the expression, return a location
        // For now, return None since we don't have span info in the AST
        let _ = (expr, offset, uri, name);
        None
    }

    fn find_definition_in_function(&self, name: &str, params: &[String], body: &[crate::ast::Stmt], offset: usize, uri: &Url) -> Option<Location> {
        let _ = (name, params, body, offset, uri);
        None
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
                    contents: HoverContents::Scalar(MarkedString::String(format!("**let {}**: {}", name, ty))),
                    range: None,
                });
            }
            Function { name, params, .. } => {
                let params_str = params.join(", ");
                return Some(Hover {
                    contents: HoverContents::Scalar(MarkedString::String(format!("**fn {}({})**", name, params_str))),
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
        for kw in ["let", "fn", "if", "else", "while", "for", "return", "break", "continue", "import", "and", "or", "not"] {
            items.push(CompletionItem {
                label: kw.to_string(),
                kind: Some(CompletionItemKind::KEYWORD),
                ..Default::default()
            });
        }

        // Builtins
        for builtin in crate::builtins::NAMES {
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

    fn collect_symbols_from_stmt(&self, stmt: &crate::ast::Stmt, symbols: &mut Vec<DocumentSymbol>) {
        use crate::ast::Stmt::*;
        match stmt {
            Let { name, .. } => {
                symbols.push(DocumentSymbol {
                    name: name.clone(),
                    kind: SymbolKind::VARIABLE,
                    range: Range::default(),
                    selection_range: Range::default(),
                    detail: Some("let".to_string()),
                    children: None,
                    deprecated: None,
                    tags: None,
                });
            }
            Function { name, params, body, .. } => {
                let mut children = Vec::new();
                // Add parameters as children
                for param in params {
                    children.push(DocumentSymbol {
                        name: param.clone(),
                        kind: SymbolKind::VARIABLE,
                        range: Range::default(),
                        selection_range: Range::default(),
                        detail: Some("parameter".to_string()),
                        children: None,
                        deprecated: None,
                        tags: None,
                    });
                }
                // Collect nested symbols from body
                for s in body {
                    self.collect_symbols_from_stmt(s, &mut children);
                }
                
                symbols.push(DocumentSymbol {
                    name: name.clone(),
                    kind: SymbolKind::FUNCTION,
                    range: Range::default(),
                    selection_range: Range::default(),
                    detail: Some(format!("fn({})", params.join(", "))),
                    children: Some(children),
                    deprecated: None,
                    tags: None,
                });
            }
            If { then_branch, else_branch, .. } => {
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

    fn collect_completions_from_stmt(&self, stmt: &crate::ast::Stmt, items: &mut Vec<CompletionItem>) {
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
            If { then_branch, else_branch, .. } => {
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
}

#[tower_lsp::async_trait]
impl LanguageServer for NectLanguageServer {
    async fn initialize(&self, _: InitializeParams) -> Result<InitializeResult> {
        Ok(InitializeResult {
            capabilities: ServerCapabilities {
                text_document_sync: Some(TextDocumentSyncCapability::Kind(TextDocumentSyncKind::FULL)),
                hover_provider: Some(HoverProviderCapability::Simple(true)),
                definition_provider: Some(OneOf::Left(true)),
                references_provider: Some(OneOf::Left(true)),
                completion_provider: Some(CompletionOptions {
                    trigger_characters: Some(vec![".".to_string(), "(".to_string()]),
                    ..Default::default()
                }),
                document_symbol_provider: Some(OneOf::Left(true)),
                diagnostic_provider: Some(DiagnosticServerCapabilities::Options(DiagnosticOptions {
                    identifier: Some("nect".to_string()),
                    ..Default::default()
                })),
                ..Default::default()
            },
            server_info: Some(ServerInfo {
                name: "nect-lsp".to_string(),
                version: Some("0.1.0".to_string()),
            }),
        })
    }

    async fn initialized(&self, _: InitializedParams) {
        self.inner.client.log_message(MessageType::INFO, "Nect LSP server initialized").await;
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
            
            self.inner.update_document(&uri, version, text.clone()).await;
            
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

    async fn goto_definition(&self, params: GotoDefinitionParams) -> Result<Option<GotoDefinitionResponse>> {
        let uri = params.text_document_position_params.text_document.uri;
        let position = params.text_document_position_params.position;
        
        if let Some(doc) = self.inner.get_document(&uri).await {
            if let Some(loc) = self.find_definition(&doc, position) {
                return Ok(Some(GotoDefinitionResponse::Scalar(loc)));
            }
        }
        Ok(None)
    }

    async fn references(&self, params: ReferenceParams) -> Result<Option<Vec<Location>>> {
        let uri = params.text_document_position.text_document.uri;
        let position = params.text_document_position.position;
        
        if let Some(doc) = self.inner.get_document(&uri).await {
            let offset = match self.position_to_offset(&doc.text, position) {
                Some(o) => o,
                None => return Ok(None),
            };
            let lexer = Lexer::new(&doc.text);
            let tokens = match lexer.tokenize() {
                Ok(t) => t,
                Err(_) => return Ok(None),
            };
            let parser = Parser::new(tokens);
            let stmts = match parser.parse() {
                Ok(s) => s,
                Err(_) => return Ok(None),
            };
            
            let mut locations = Vec::new();
            for stmt in &stmts {
                Self::find_references_in_stmt_static(stmt, offset, &uri, &mut locations);
            }
            
            if !locations.is_empty() {
                return Ok(Some(locations));
            }
        }
        Ok(None)
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

    async fn document_symbol(&self, params: DocumentSymbolParams) -> Result<Option<DocumentSymbolResponse>> {
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

impl NectLanguageServer {
    fn find_references_in_stmt_static(stmt: &crate::ast::Stmt, offset: usize, uri: &Url, locations: &mut Vec<Location>) {
        // Since we don't have span info, we can't do precise reference finding
        // This is a placeholder
        let _ = (stmt, offset, uri, locations);
    }
}