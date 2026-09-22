import * as vscode from 'vscode';
import { LanguageClient, LanguageClientOptions, ServerOptions, TransportKind } from 'vscode-languageclient/node';

let client: LanguageClient;

export function activate(context: vscode.ExtensionContext) {
  console.log('Nect Language Support is now active');

  const config = vscode.workspace.getConfiguration('nect.lsp');
  
  if (!config.get<boolean>('enabled', true)) {
    console.log('Nect LSP is disabled');
    return;
  }

  const serverPath = config.get<string>('path', 'nect');
  const traceLevel = config.get<string>('trace.server', 'off');

  const serverOptions: ServerOptions = {
    command: serverPath,
    args: ['lsp'],
    transport: TransportKind.stdio,
    options: {
      env: {
        ...process.env,
        RUST_LOG: traceLevel === 'verbose' ? 'debug' : 'info'
      }
    }
  };

  const clientOptions: LanguageClientOptions = {
    documentSelector: [
      { scheme: 'file', language: 'nect' },
      { scheme: 'untitled', language: 'nect' }
    ],
    synchronize: {
      fileEvents: vscode.workspace.createFileSystemWatcher('**/*.nct')
    },
    initializationOptions: {},
    middleware: {
      provideHover: async (document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken, next: (doc: vscode.TextDocument, pos: vscode.Position, tok: vscode.CancellationToken) => vscode.ProviderResult<vscode.Hover>) => {
        return next(document, position, token);
      },
      provideDefinition: async (document: vscode.TextDocument, position: vscode.Position, token: vscode.CancellationToken, next: (doc: vscode.TextDocument, pos: vscode.Position, tok: vscode.CancellationToken) => vscode.ProviderResult<vscode.Definition | vscode.DefinitionLink[]>) => {
        return next(document, position, token);
      },
      provideReferences: async (document: vscode.TextDocument, position: vscode.Position, context: vscode.ReferenceContext, token: vscode.CancellationToken, next: (doc: vscode.TextDocument, pos: vscode.Position, ctx: vscode.ReferenceContext, tok: vscode.CancellationToken) => vscode.ProviderResult<vscode.Location[]>) => {
        return next(document, position, context, token);
      },
      provideCompletionItem: async (document: vscode.TextDocument, position: vscode.Position, context: vscode.CompletionContext, token: vscode.CancellationToken, next: (doc: vscode.TextDocument, pos: vscode.Position, ctx: vscode.CompletionContext, tok: vscode.CancellationToken) => vscode.ProviderResult<vscode.CompletionItem[] | vscode.CompletionList>) => {
        return next(document, position, context, token);
      }
    }
  };

  client = new LanguageClient(
    'nectLanguageServer',
    'Nect Language Server',
    serverOptions,
    clientOptions
  );

  client.onDidChangeState((e) => {
    console.log(`Nect LSP state changed: ${e.newState}`);
  });

  client.onNotification('$/logTrace', (params: any) => {
    if (traceLevel !== 'off') {
      console.log(`[Nect LSP] ${params.message}`);
    }
  });

  client.start().then(() => {
    console.log('Nect LSP client started');
  }).catch((err) => {
    console.error('Failed to start Nect LSP client:', err);
    vscode.window.showErrorMessage(`Failed to start Nect Language Server: ${err.message}`);
  });

  context.subscriptions.push(
    vscode.commands.registerCommand('nect.restartLsp', () => {
      client.stop().then(() => client.start());
    })
  );

  context.subscriptions.push(
    vscode.commands.registerCommand('nect.showOutput', () => {
      client.outputChannel.show();
    })
  );
}

export function deactivate(): Thenable<void> | undefined {
  if (!client) {
    return undefined;
  }
  return client.stop();
}