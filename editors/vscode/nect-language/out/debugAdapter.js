"use strict";
/**
 * Debug adapter wiring for Nect.
 *
 * The `nect` binary speaks the Debug Adapter Protocol itself (`nect dap`), so
 * there is no protocol code here: VS Code is told to run the binary and talk to
 * it directly. That keeps the adapter and the `nect debug` terminal front end on
 * exactly the same engine, so a breakpoint behaves the same way in an editor as
 * it does in a terminal.
 */
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
exports.activateDebugAdapter = activateDebugAdapter;
const fs = __importStar(require("fs"));
const os = __importStar(require("os"));
const path = __importStar(require("path"));
const vscode = __importStar(require("vscode"));
/** The `nect` executable, honouring the same setting the LSP client uses. */
function nectExecutable() {
    const configured = vscode.workspace
        .getConfiguration('nect.lsp')
        .get('path', 'nect');
    return configured && configured.length > 0 ? configured : 'nect';
}
/**
 * The file the adapter should load.
 *
 * An editor buffer that has never been saved has no path on disk, and the
 * adapter reads a program from a file. Rather than refusing to debug it, the
 * buffer's text is written to a temporary file, which is removed when the session
 * ends.
 */
function programForSession(session, cleanup) {
    const configured = session.configuration.program;
    if (!configured) {
        return undefined;
    }
    if (fs.existsSync(configured)) {
        return configured;
    }
    const document = vscode.workspace.textDocuments.find((candidate) => candidate.uri.fsPath === configured || candidate.uri.toString() === configured);
    if (!document) {
        return undefined;
    }
    const temporary = path.join(fs.mkdtempSync(path.join(os.tmpdir(), 'nect-dap-')), path.basename(configured) || 'main.nct');
    fs.writeFileSync(temporary, document.getText(), 'utf8');
    cleanup.push(() => {
        try {
            fs.rmSync(path.dirname(temporary), { recursive: true, force: true });
        }
        catch {
            // A temporary file that outlives the session is not worth failing over.
        }
    });
    return temporary;
}
function activateDebugAdapter(context) {
    const cleanup = [];
    context.subscriptions.push(vscode.debug.registerDebugAdapterDescriptorFactory('nect', {
        createDebugAdapterDescriptor(session) {
            const program = programForSession(session, cleanup);
            if (!program) {
                // VS Code surfaces this as a failed launch, which is clearer than
                // starting an adapter with nothing to debug.
                void vscode.window.showErrorMessage('Nect: could not find the program to debug. Save the file first, or set ' +
                    '"program" in your launch configuration.');
                return new vscode.DebugAdapterExecutable(nectExecutable(), [
                    'dap',
                    path.join(os.tmpdir(), 'nect-dap-missing.nct'),
                ]);
            }
            return new vscode.DebugAdapterExecutable(nectExecutable(), ['dap', program]);
        },
    }));
    context.subscriptions.push(new vscode.Disposable(() => {
        for (const dispose of cleanup) {
            dispose();
        }
        cleanup.length = 0;
    }));
}
//# sourceMappingURL=debugAdapter.js.map