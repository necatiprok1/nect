# Nect for VS Code

Source for Nect syntax highlighting, language-server integration, and debugging.
The extension's generated JavaScript and VSIX packages are not stored in this
repository.

## Requirements

- VS Code 1.80 or newer.
- Nect on `PATH`, or an explicit executable path in `nect.lsp.path`.
- A Nect build with the `lsp` feature (the `full` distribution includes it) for
  language-server features. Syntax highlighting does not require the server.
- Node.js and npm only when building this extension from source.

## Build and install

From `editors/vscode/nect-language`:

```sh
npm ci
npm run compile
npx @vscode/vsce package
code --install-extension nect-language-0.1.0.vsix
```

The packaging tool may be downloaded by `npx`. `vscode:prepublish` recompiles the
sources before packaging. For development, compile first, then press F5 in
VS Code to launch an Extension Development Host.

## Features

- Highlighting and bracket/comment configuration for `.nct` files.
- LSP diagnostics, hover, signature help, definition/reference navigation,
  completion, semantic highlighting, and document symbols.
- Debug breakpoints, continue, top-level-statement stepping, and variable
  inspection. Stepping is not function-level step-in/step-out.

| Setting | Default | Purpose |
| --- | --- | --- |
| `nect.lsp.enabled` | `true` | Start the language server |
| `nect.lsp.path` | `nect` | Executable used by the language server/debug adapter |
| `nect.lsp.trace.server` | `off` | Protocol logging: `off`, `messages`, `verbose` |

Use **Nect: Restart Language Server** to restart the server, or **Nect: Show
Output Channel** to view its logs. See the project [installation guide](../../../docs/KURULUM.md)
and [language reference](../../../docs/reference.md) for Nect itself.
