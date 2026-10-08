/**
 * Debug adapter wiring for Nect.
 *
 * The `nect` binary speaks the Debug Adapter Protocol itself (`nect dap`), so
 * there is no protocol code here: VS Code is told to run the binary and talk to
 * it directly. That keeps the adapter and the `nect debug` terminal front end on
 * exactly the same engine, so a breakpoint behaves the same way in an editor as
 * it does in a terminal.
 */

import * as fs from 'fs';
import * as os from 'os';
import * as path from 'path';
import * as vscode from 'vscode';

/** The `nect` executable, honouring the same setting the LSP client uses. */
function nectExecutable(): string {
  const configured = vscode.workspace
    .getConfiguration('nect.lsp')
    .get<string>('path', 'nect');
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
function programForSession(
  session: vscode.DebugSession,
  cleanup: (() => void)[]
): string | undefined {
  const configured = (session.configuration as { program?: string }).program;
  if (!configured) {
    return undefined;
  }
  if (fs.existsSync(configured)) {
    return configured;
  }
  const document = vscode.workspace.textDocuments.find(
    (candidate) => candidate.uri.fsPath === configured || candidate.uri.toString() === configured
  );
  if (!document) {
    return undefined;
  }
  const temporary = path.join(
    fs.mkdtempSync(path.join(os.tmpdir(), 'nect-dap-')),
    path.basename(configured) || 'main.nct'
  );
  fs.writeFileSync(temporary, document.getText(), 'utf8');
  cleanup.push(() => {
    try {
      fs.rmSync(path.dirname(temporary), { recursive: true, force: true });
    } catch {
      // A temporary file that outlives the session is not worth failing over.
    }
  });
  return temporary;
}

export function activateDebugAdapter(context: vscode.ExtensionContext): void {
  const cleanup: (() => void)[] = [];

  context.subscriptions.push(
    vscode.debug.registerDebugAdapterDescriptorFactory('nect', {
      createDebugAdapterDescriptor(session: vscode.DebugSession) {
        const program = programForSession(session, cleanup);
        if (!program) {
          // VS Code surfaces this as a failed launch, which is clearer than
          // starting an adapter with nothing to debug.
          void vscode.window.showErrorMessage(
            'Nect: could not find the program to debug. Save the file first, or set ' +
              '"program" in your launch configuration.'
          );
          return new vscode.DebugAdapterExecutable(nectExecutable(), [
            'dap',
            path.join(os.tmpdir(), 'nect-dap-missing.nct'),
          ]);
        }
        return new vscode.DebugAdapterExecutable(nectExecutable(), ['dap', program]);
      },
    })
  );

  context.subscriptions.push(
    new vscode.Disposable(() => {
      for (const dispose of cleanup) {
        dispose();
      }
      cleanup.length = 0;
    })
  );
}
