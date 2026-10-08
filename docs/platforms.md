# Platform Support

## Official Support Matrix

| Platform               | Architecture | Status      | JIT Support | Notes                          |
| ---------------------- | ------------ | ----------- | ----------- | ------------------------------ |
| Linux (glibc)          | x64 (amd64)  | Official    | Yes         | Primary development platform   |
| Linux (glibc)          | ARM64        | Official    | Yes         | Tested on CI                   |
| macOS                  | ARM64        | Official    | Yes         | Primary macOS platform         |
| macOS                  | x64          | Official    | Yes         | Tested on CI                   |
| Windows                | x64          | Official    | Yes         | Uses MSVC, tested on CI        |
| Windows                | ARM64        | Experimental | Yes       | Limited testing                |
| Linux (musl)           | x64          | Experimental | Yes       | Static linking                 |
| Linux (musl)           | ARM64        | Experimental | Yes       | Static linking                 |

## Release Artifacts

Nect releases are distributed as platform-specific tarballs containing
the `nect` binary. Each release includes:

- `nect-<triple>.tar.gz` — the lean binary (Windows: `.zip`)
- `nect-<triple>-full.tar.gz` — the full binary (Windows: `.zip`)
- `SHA256SUMS` — SHA-256 checksums for every archive above
- `install.sh` / `install.ps1` — the one-line installers

The archive name carries no version on purpose: the installer fetches
`releases/latest/download/nect-<triple>.tar.gz`, so the name has to stay the
same across releases for that URL to keep resolving. The version is inside the
archive, in a `VERSION` file, and implied by which release you downloaded from.

### Two builds

| | lean | full |
| --- | --- | --- |
| Binary size | ~3 MB | ~7 MB |
| Crates compiled | ~50 | ~235 |
| Contains | the language: VM, JIT, C backend, core built-ins | lean, plus LSP, package manager, FFI |
| Does not contain | HTTP client/server, SQLite, GUI | — |

Neither includes the GUI, HTTP, or SQLite built-ins; those are opt-in cargo
features (`gui`, `net`, `server`, `db`) because together they are the large
majority of the dependency tree. See [KURULUM.md](KURULUM.md), which is in
Turkish.

### Verifying a Release

After downloading, verify the checksum:

```sh
sha256sum -c SHA256SUMS
```

## Installation

### Using the install script (recommended)

macOS and Linux:

```sh
curl -fsSL https://get.nect-lang.org/install.sh | sh
curl -fsSL https://get.nect-lang.org/install.sh | sh -s -- --full
```

Windows (PowerShell):

```powershell
irm https://get.nect-lang.org/install.ps1 | iex
irm https://get.nect-lang.org/install.ps1 | iex -Full
```

Both scripts download the archive, verify it against `SHA256SUMS`, install it to
a directory that needs no elevation (`~/.local/bin`, or
`%LOCALAPPDATA%\Nect\bin` on Windows), and add that directory to `PATH` —
appending a marked line to the shell profile on Unix, editing the *user* `PATH`
on Windows. Re-running is safe: an already-correct `PATH` is left alone.

### Manual download

Download the appropriate tarball from
[GitHub Releases](https://github.com/nect-lang/nect/releases), then:

```sh
tar -xzf nect-x86_64-unknown-linux-gnu.tar.gz
chmod +x nect-x86_64-unknown-linux-gnu/nect
sudo install -m 755 nect-x86_64-unknown-linux-gnu/nect /usr/local/bin/nect
```

### From source

```sh
git clone https://github.com/nect-lang/nect.git
cd nect
cargo install --path .              # lean
cargo install --path . --features full
```

### Using a package manager

Nect is available in the following package repositories:

- Homebrew (macOS): `brew install nect-lang/nect/nect`
- Scoop (Windows): `scoop bucket add nect https://github.com/nect-lang/nect-bucket`
- Nix: `nix install nect`

## Diagnostics

Run `nect doctor` to verify your installation and toolchain:

```sh
nect doctor
```

This checks:
- Platform and architecture
- Nect compiler version
- Rust compiler availability
- C compiler (required for `nect build`)
- JIT/Cranelift availability

## Cross-Compilation

Nect supports cross-compilation for C backends. Use the `--target` flag
with `nect build`:

```sh
nect build main.nct --target aarch64-unknown-linux-gnu
```

For cross-compilation, a C cross-compiler matching the target triple
is required. Use `--cc` to specify a custom compiler:

```sh
nect build main.nct \
  --target aarch64-unknown-linux-gnu \
  --cc aarch64-linux-gnu-gcc
```
