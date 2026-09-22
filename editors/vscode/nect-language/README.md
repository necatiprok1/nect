# Nect Language Support for VS Code

This extension provides language support for the Nect programming language, including:

- **Syntax highlighting** for `.nct` files
- **Language Server Protocol (LSP)** integration for:
  - Diagnostics (parse errors, type errors)
  - Hover information (types, function signatures)
  - Go to definition
  - Find references
  - Code completion (keywords, built-ins, local variables/functions)
- **Snippets** for common patterns

## Installation

### From VSIX (Recommended)
```bash
# Build the extension
cd editors/vscode/nect-language
npm install
npm run compile
vsce package

# Install in VS Code
code --install-extension nect-language-0.1.0.vsix
```

### From Source (Development)
```bash
cd editors/vscode/nect-language
npm install
npm run compile
# Press F5 in VS Code to launch Extension Development Host
```

## Requirements

- **Nect CLI** (`nect`) must be installed and available in PATH
- The LSP server is started via `nect lsp` command

## Configuration

| Setting | Type | Default | Description |
|---------|------|---------|-------------|
| `nect.lsp.enabled` | boolean | `true` | Enable/disable the Nect Language Server |
| `nect.lsp.path` | string | `"nect"` | Path to the nect executable |
| `nect.lsp.trace.server` | string | `"off"` | Trace level: `off`, `messages`, `verbose` |

## Features

### Syntax Highlighting
- Keywords: `let`, `fn`, `if`, `else`, `while`, `for`, `return`, `break`, `continue`, `import`
- Logical operators: `and`, `or`, `not`
- Built-in functions: 64 functions including `print`, `len`, `push`, `map`, `filter`, etc.
- String interpolation: `${expression}`
- Numbers with digit separators: `1_000_000`
- Exponent literals: `1.5e2`
- Comments: `//`, `#`, `/* */`

### LSP Features
- **Real-time diagnostics**: Parse errors shown as red squiggles
- **Hover**: Type information for variables and function signatures
- **Go to Definition**: Jump to `let` and `fn` declarations
- **Find References**: Find usages of variables/functions
- **Completion**: Context-aware suggestions for keywords, built-ins, and local symbols

## Commands

| Command | Description |
|---------|-------------|
| `Nect: Restart Language Server` | Restart the LSP server |
| `Nect: Show Output Channel` | Open the LSP output channel |

## Language Features

Nect is a Python-like language with:
- `let` bindings
- Functions with `fn`
- Control flow: `if/else`, `while`, `for in`
- Arrays, maps, strings
- 64 built-in functions
- String interpolation
- Optional parentheses on `if`/`while`

## Example

```nect
// Variables
let name = "Nect"
let numbers = [1, 2, 3, 4, 5]

// Functions
fn greet(who) {
    print("Hello, " + who)
}

// Control flow
if len(name) > 0 {
    greet(name)
} else {
    print("No name")
}

// Loops
for n in numbers {
    if n % 2 == 0 {
        continue
    }
    print(n)
}

// String interpolation
print("Length is ${len(name)}")
```

## License

MIT