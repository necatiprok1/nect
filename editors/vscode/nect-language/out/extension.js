"use strict";
var __createBinding = (this && this.__createBinding) || (Object.create ? (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    var desc = Object.getOwnPropertyDescriptor(m, k);
    if (!desc || ("get" in desc ? !m.__esModule : desc.writable || desc.configurable)) {
      desc = { enumerable: true, get: function() { return m[k]; } };
    }
    Object.defineProperty(o, k2, desc);
}) : (function(o, m, k, k2) {
    if (k2 === undefined) k2 = k;
    o[k2] = m[k];
}));
var __setModuleDefault = (this && this.__setModuleDefault) || (Object.create ? (function(o, v) {
    Object.defineProperty(o, "default", { enumerable: true, value: v });
}) : function(o, v) {
    o["default"] = v;
});
var __importStar = (this && this.__importStar) || (function () {
    var ownKeys = function(o) {
        ownKeys = Object.getOwnPropertyNames || function (o) {
            var ar = [];
            for (var k in o) if (Object.prototype.hasOwnProperty.call(o, k)) ar[ar.length] = k;
            return ar;
        };
        return ownKeys(o);
    };
    return function (mod) {
        if (mod && mod.__esModule) return mod;
        var result = {};
        if (mod != null) for (var k = ownKeys(mod), i = 0; i < k.length; i++) if (k[i] !== "default") __createBinding(result, mod, k[i]);
        __setModuleDefault(result, mod);
        return result;
    };
})();
Object.defineProperty(exports, "__esModule", { value: true });
exports.activate = activate;
exports.deactivate = deactivate;
const vscode = __importStar(require("vscode"));
const node_1 = require("vscode-languageclient/node");
const debugAdapter_1 = require("./debugAdapter");
let client;
function activate(context) {
    console.log('Nect Language Support is now active');
    const config = vscode.workspace.getConfiguration('nect.lsp');
    if (!config.get('enabled', true)) {
        console.log('Nect LSP is disabled');
        return;
    }
    const serverPath = config.get('path', 'nect');
    const traceLevel = config.get('trace.server', 'off');
    const serverOptions = {
        command: serverPath,
        args: ['lsp'],
        transport: node_1.TransportKind.stdio,
        options: {
            env: {
                ...process.env,
                RUST_LOG: traceLevel === 'verbose' ? 'debug' : 'info'
            }
        }
    };
    const clientOptions = {
        documentSelector: [
            { scheme: 'file', language: 'nect' },
            { scheme: 'untitled', language: 'nect' }
        ],
        synchronize: {
            fileEvents: vscode.workspace.createFileSystemWatcher('**/*.nct')
        },
        initializationOptions: {},
        middleware: {
            provideHover: async (document, position, token, next) => {
                return next(document, position, token);
            },
            provideDefinition: async (document, position, token, next) => {
                return next(document, position, token);
            },
            provideReferences: async (document, position, context, token, next) => {
                return next(document, position, context, token);
            },
            provideCompletionItem: async (document, position, context, token, next) => {
                return next(document, position, context, token);
            }
        }
    };
    client = new node_1.LanguageClient('nectLanguageServer', 'Nect Language Server', serverOptions, clientOptions);
    client.onDidChangeState((e) => {
        console.log(`Nect LSP state changed: ${e.newState}`);
    });
    client.onNotification('$/logTrace', (params) => {
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
    context.subscriptions.push(vscode.commands.registerCommand('nect.restartLsp', () => {
        client.stop().then(() => client.start());
    }));
    context.subscriptions.push(vscode.commands.registerCommand('nect.showOutput', () => {
        client.outputChannel.show();
    }));
    // Debugging is registered whether or not the language server is enabled: a
    // user may want to step through a program with no LSP running at all.
    (0, debugAdapter_1.activateDebugAdapter)(context);
}
function deactivate() {
    if (!client) {
        return undefined;
    }
    return client.stop();
}
//# sourceMappingURL=extension.js.map