# Nect 1.x Compatibility Policy

> **Status:** Official policy, version 1.0
>
> This document defines what constitutes a breaking change in Nect, how
> compatibility is maintained across 1.x releases, and the rules for the
> package ecosystem. It is the authoritative reference for whether a proposed
> change requires a major version bump.

## 1. Versioning

Nect follows [Semantic Versioning 2.0.0](https://semver.org/):

- **MAJOR** version for incompatible changes to the language, standard library,
  or public CLI contract.
- **MINOR** version for backward-compatible additions: new syntax that does not
  change existing parsing, new built-in functions, new CLI subcommands, new
  environment variables.
- **PATCH** version for backward-compatible bug fixes, performance improvements,
  and documentation updates.

Pre-1.0 behavior: the original Phase 1–7 roadmap is complete. Nect 1.0 freezes
the language surface described in `docs/spec/spec.md`.

## 2. What is the "public API"

The Nect 1.x public API includes:

1. **Language syntax** — all constructs described in `docs/spec/spec.md` §2
   (Grammar). Syntax that is not described there is not yet stable.
2. **Language semantics** — all behaviors, error messages, and formatting rules
   in `docs/spec/spec.md`.
3. **Built-in functions** — every name in `NAMES` (`src/builtins.rs:614`) and
   their documented signatures, return types, and error conditions.
4. **CLI interface** — `nect run`, `nect check`, `nect disasm`, `nect build`,
   `nect --version`, `nect --help`, and the `NECT_NO_JIT` / `CC` environment
   variables.
5. **Exit codes** — `0` for success, `1` for any error, for all commands.
6. **Standard library modules** — `import "std/..."` modules embedded in the
   binary via `STDLIB` (`src/cli/mod.rs:68`).
7. **Package format** — `nect.toml` and `nect.lock` manifests (see §6).

The only intentional behavioral divergences between engines are the five
documented in `docs/spec/spec.md` §12. Any other divergence is a bug.

## 3. Breaking changes to the language

A change is **breaking** (requires a MAJOR version bump) if it:

### 3.1 Syntax

- Removes or renames a keyword.
- Changes operator precedence or associativity in a way that alters the parse of
  existing code.
- Makes previously-valid syntax a parse error (other than removing an ambiguous
  or undefined edge case that was never tested).
- Changes the meaning of `if`/`while`/`for`/`fn`/`let`/`return` constructs.

### 3.2 Semantics

- Changes the result or error of any operation on values that previously
  succeeded or errored deterministically.
- Changes the formatting of any value (e.g., number printing, string escaping
  in arrays, map key quoting).
- Changes truthiness rules.
- Alters scope resolution (e.g., making `if` bodies into scopes where they were
  not, or vice versa).
- Changes index evaluation order (e.g., evaluating a compound-assignment index
  expression more than once).
- Changes the set of JIT-eligible functions in a way that alters observable
  behavior. (JIT rejection reasons may change; they are not the public contract.)

### 3.3 Standard library

- Renames, removes, or changes the arity of a built-in function.
- Changes a built-in's return type or error message on a previously-valid input.
- Changes the argument types accepted by a built-in.
- Changes the maximum sizes enforced by `range` / `repeat` (currently 10,000,000).

### 3.4 CLI

- Renames or removes a subcommand.
- Removes or renames an environment variable.
- Changes an exit code.
- Changes the meaning of an existing flag (e.g., `--interp`).

## 4. Non-breaking changes

The following MAY be done in a MINOR release:

- Adding a new keyword that only appears in a position where it was previously a
  syntax error (with no ambiguity introduced).
- Adding new built-in functions (existing names are reserved; new names do not
  conflict).
- Adding new CLI subcommands.
- Adding new optional parameters to built-ins (existing calls remain valid).
- Changing JIT rejection reasons or adding new ones (does not affect programs
  that don't use native compilation).
- Changing the set of code the JIT/AOT translates (as long as behavior matches
  the bytecode VM exactly).
- Changing `disasm` output format (not a public API).
- Performance improvements, including new optimizations that preserve semantics.

## 5. Engine parity guarantee

The bytecode VM, the reference interpreter, and the JIT must produce identical
observable behavior (stdout, stderr, exit status) for all programs that the
interpreter can run. This is enforced by `tests/differential_tests.rs`. A
change that introduces a new divergence must either:

- Fix the divergence so the engines agree, or
- Add the divergence to the `documented_divergences` test and to
  `docs/spec/spec.md` §12.

The AOT (C backend) must match the VM for all programs it accepts; this is
enforced by `tests/aot_tests.rs`.

## 6. Package compatibility

### 6.1 Manifest format

The package manifest (`nect.toml`) follows the [TOML](https://toml.io/) format.
The minimal manifest declares:

```toml
[package]
name = "my-package"
version = "1.0.0"
```

A `[dependencies]` table maps package names to version requirements:

```toml
[dependencies]
other-pkg = "1.2.0"          # exact or caret requirement
some-lib = "^2.0"            # compatible-with
```

Version requirements follow SemVer caret (`^1.2.3`) and tilde (`~1.2.3`)
semantics, matching Cargo's conventions.

### 6.2 Lock file

`nect.lock` records exact resolved versions for reproducible builds. It is
committed for applications and optional for libraries. The lock file format is
stable within a MAJOR version.

### 6.3 Dependency resolution

- Resolution is deterministic: the same manifest set + lock file always
  resolves to the same dependency graph.
- The resolver prefers the highest compatible version that does not violate any
  SemVer constraint.
- Conflicts (incompatible version requirements for the same package) are errors
  with a clear message.

### 6.4 Registry

Packages are published to and downloaded from a registry. The registry contract:

- Package names are non-empty, lowercase, and use hyphens/underscores.
- Version numbers follow SemVer 2.0.0.
- Published packages are immutable: a version, once published, is never deleted
  or modified. Typos are corrected by publishing a new version (yanked versions
  are excluded from resolution but not removed from the registry).

### 6.5 Integrity and security

- Every downloaded package is verified against a SHA-256 checksum from the
  registry. A checksum mismatch is a hard error.
- Future: package signing (see Phase 11/20). Until signing is implemented,
- checksum verification is the only integrity guarantee.

## 7. Feature lifecycle

### 7.1 Experimental features

Features that are not yet stable may be gated behind a flag or a special naming
convention (e.g., prefixed with `_`). Experimental features:

- May change or be removed in a PATCH or MINOR release.
- Are not covered by the engine parity guarantee.
- Are documented as experimental in `docs/spec/spec.md`.

The async/await, HTTP, database, GUI, and tensor built-ins are currently
considered experimental: they are present in `src/builtins.rs` but their
behavior is not pinned by the differential tests.

### 7.2 Deprecation

- A deprecation notice appears in the deprecation log and in `nect --help` /
  `nect check` warnings before removal.
- Deprecation does not break existing code (deprecated features remain functional).
- Removal requires a MAJOR version bump.

## 8. Release process

1. **Feature freeze** — all planned MINOR features are merged and tested.
2. **CI pass** — all tests, lint, formatting, and differential/AOT checks pass
   on all supported platforms.
3. **Changelog** — `CHANGELOG.md` is updated with user-facing changes, noting
   any breaking changes with migration guidance.
4. **Tagging** — a git tag `vMAJOR.MINOR.PATCH` is created.
5. **Artifacts** — release binaries are built for all supported platforms
   (Linux x64, Linux ARM64, macOS x64, macOS ARM64, Windows x64), with
   SHA-256 checksums.
6. **Verification** — at least one release artifact is downloaded and tested
   on a clean environment before the GitHub release is published.

## 9. What changes without notice

The following are NOT part of the public API and may change at any time:

- Internal bytecode/opcode format (the `disasm` output format).
- JIT rejection reason strings (used only for diagnostics, not behavior).
- Internal AST node structure (`src/ast/mod.rs`).
- Implementation details of the Cranelift JIT (`src/jit/`).
- Implementation details of the C backend (`src/aot/`).
- The format of `nect.lock` (it is regenerated if absent).
- The source layout of `std/` modules (their public behavior is stable, but
  their internal structure may change).
- Error message wording (unless pinned by a test in
  `tests/vm_output_tests.rs`).

Error message text IS the public contract only when pinned by
`tests/vm_output_tests.rs` or `tests/differential_tests.rs`. All other error
messages may be reworded in any release.
