use lsp_types::*;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_lsp::{Client, LspService, Server};

pub mod documents;
pub mod index;
pub mod server;

use server::NectLanguageServer;

pub async fn start_lsp_server() -> Result<(), Box<dyn std::error::Error>> {
    let stdin = tokio::io::stdin();
    let stdout = tokio::io::stdout();

    let (service, socket) = LspService::new(NectLanguageServer::new);
    Server::new(stdin, stdout, socket).serve(service).await;

    Ok(())
}

#[derive(Debug, Clone)]
pub struct DocumentState {
    pub uri: Url,
    pub version: i32,
    pub text: String,
    pub diagnostics: Vec<Diagnostic>,
}

impl DocumentState {
    pub fn new(uri: Url, version: i32, text: String) -> Self {
        Self {
            uri,
            version,
            text,
            diagnostics: Vec::new(),
        }
    }

    pub fn update(&mut self, version: i32, text: String) {
        self.version = version;
        self.text = text;
    }
}

#[derive(Debug)]
pub struct NectLanguageServerInner {
    client: Client,
    documents: Arc<RwLock<HashMap<Url, DocumentState>>>,
}

impl NectLanguageServerInner {
    pub fn new(client: Client) -> Self {
        Self {
            client,
            documents: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn get_document(&self, uri: &Url) -> Option<DocumentState> {
        let docs = self.documents.read().await;
        docs.get(uri).cloned()
    }

    pub async fn insert_document(&self, doc: DocumentState) {
        let mut docs = self.documents.write().await;
        docs.insert(doc.uri.clone(), doc);
    }

    pub async fn remove_document(&self, uri: &Url) {
        let mut docs = self.documents.write().await;
        docs.remove(uri);
    }

    pub async fn update_document(&self, uri: &Url, version: i32, text: String) {
        let mut docs = self.documents.write().await;
        if let Some(doc) = docs.get_mut(uri) {
            doc.update(version, text);
        }
    }

    pub async fn publish_diagnostics(&self, uri: &Url, diagnostics: Vec<Diagnostic>) {
        let mut docs = self.documents.write().await;
        if let Some(doc) = docs.get_mut(uri) {
            doc.diagnostics = diagnostics.clone();
        }
        self.client
            .publish_diagnostics(uri.clone(), diagnostics, None)
            .await;
    }
}
