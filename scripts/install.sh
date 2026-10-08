#!/bin/sh
# Nect installer for macOS and Linux.
#
#   curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh
#
#   curl -fsSL https://github.com/necatiprok1/nect/releases/latest/download/install.sh | sh -s -- --full
#
# Options:
#
#   --full               install the `full` build (default: the lean build)
#   --version <tag>      a release tag to pin (default: latest)
#   --install-dir <dir>  where the binary goes (default: $HOME/.local/bin)
#   --no-path            do not touch the shell profile
#   -h, --help           show this help
#
# The same settings are read from the environment, so they work without flags:
# INSTALL_DIR, NECT_VERSION, NECT_FULL=1, NECT_NO_PATH=1.
#
# The lean build is the language: about 3 MB, no GUI toolkit, no HTTP stack, no
# database engine. The full build adds the language server, the package manager,
# and the FFI. Both run the same programs; they differ in which optional
# built-ins exist. See docs/KURULUM.md.

set -eu

REPO="necatiprok1/nect"
INSTALL_DIR="${INSTALL_DIR:-$HOME/.local/bin}"
FULL="${NECT_FULL:-0}"
NO_PATH="${NECT_NO_PATH:-0}"

usage() {
    cat <<'HELP'
Usage: install.sh [--full] [--version <tag>] [--install-dir <dir>] [--no-path]

Installs a prebuilt Nect binary; no Rust or C/C++ compiler is needed.
Default: lean build in ~/.local/bin, with the shell PATH configured.
Environment: INSTALL_DIR, NECT_VERSION, NECT_FULL=1, NECT_NO_PATH=1.
HELP
}

# `sh -s -- --full` puts the flags in "$@", so they are parsed here as well as
# being read from the environment. A flag wins over its variable.
while [ $# -gt 0 ]; do
    case "$1" in
        --full) FULL=1 ;;
        --no-path) NO_PATH=1 ;;
        --version) NECT_VERSION="${2:?--version needs a tag}"; shift ;;
        --version=*) NECT_VERSION="${1#*=}" ;;
        --install-dir) INSTALL_DIR="${2:?--install-dir needs a path}"; shift ;;
        --install-dir=*) INSTALL_DIR="${1#*=}" ;;
        -h|--help) usage; exit 0 ;;
        *) printf 'error: unknown option %s\n\n' "$1" >&2; usage >&2; exit 1 ;;
    esac
    shift
done

# --- what to fetch ----------------------------------------------------------

# The archive name carries no version, so the "latest" URL is stable across
# releases. The version comes back in the archive and from SHA256SUMS.
triple() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
        Darwin)
            case "$arch" in
                arm64|aarch64) echo "aarch64-apple-darwin" ;;
                x86_64) echo "x86_64-apple-darwin" ;;
                *) echo "Unsupported architecture: $arch" >&2; exit 1 ;;
            esac
            ;;
        Linux)
            case "$arch" in
                aarch64|arm64) echo "aarch64-unknown-linux-gnu" ;;
                x86_64|amd64) echo "x86_64-unknown-linux-gnu" ;;
                *)
                    echo "Unsupported architecture: $arch" >&2
                    echo "Build from source instead: see docs/KURULUM.md" >&2
                    exit 1
                    ;;
            esac
            ;;
        *)
            echo "Unsupported system: $os" >&2
            echo "On Windows use install.ps1; on other systems build from source." >&2
            exit 1
            ;;
    esac
}

TRIPLE=$(triple)

VARIANT=""
[ "$FULL" = "1" ] && VARIANT="-full"

if [ -n "${NECT_VERSION:-}" ]; then
    LABEL="v${NECT_VERSION#v}"
    BASE="https://github.com/$REPO/releases/download/$LABEL"
else
    BASE="https://github.com/$REPO/releases/latest/download"
    LABEL="latest"
fi

ARCHIVE="nect-$TRIPLE$VARIANT.tar.gz"
URL="$BASE/$ARCHIVE"

say() { printf '%s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

need() {
    command -v "$1" >/dev/null 2>&1 || die "'$1' is required but not installed. $2"
}

has() { command -v "$1" >/dev/null 2>&1; }

# A checksum is only worth having if it is actually checked, so the verifier is
# required rather than best-effort. macOS has `shasum`, Linux `sha256sum`.
if has sha256sum; then
    CHECK="sha256sum"
elif has shasum; then
    CHECK="shasum -a 256"
else
    die "neither sha256sum nor shasum is available, so the download cannot be verified"
fi

if [ "$FULL" = "1" ]; then
    BUILD=full
else
    BUILD=lean
fi
say "Nect ($LABEL, $BUILD build) for $TRIPLE"

need curl "On Debian/Ubuntu: apt install curl. On Fedora: dnf install curl."
need tar  "On Debian/Ubuntu: apt install tar."
# --- download ---------------------------------------------------------------

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT
trap 'exit 1' INT TERM

say "Downloading $URL"
curl -fsSL --progress-bar "$URL" -o "$TMP/$ARCHIVE" \
    || die "download failed. Check the URL, or build from source (docs/KURULUM.md)"

# --- verify -----------------------------------------------------------------
#
# A checksum is only worth having if it is actually checked. SHA256SUMS sits next
# to the archive on the release, and covers both variants.

SUMS_URL="$BASE/SHA256SUMS"
curl -fsSL "$SUMS_URL" -o "$TMP/SHA256SUMS" \
    || die "SHA256SUMS could not be fetched; refusing an unverified installation"
EXPECTED=$(awk -v f="$ARCHIVE" '$2 == f || $2 == "*" f {print $1; exit}' "$TMP/SHA256SUMS")
[ -n "$EXPECTED" ] || die "SHA256SUMS has no entry for $ARCHIVE"
# shellcheck disable=SC2086
ACTUAL=$($CHECK "$TMP/$ARCHIVE" | awk '{print $1}')
[ "$ACTUAL" = "$EXPECTED" ] || die "checksum mismatch: the download is corrupt or tampered with"
say "Checksum verified."

# --- install ----------------------------------------------------------------

mkdir "$TMP/unpack"
tar -xzf "$TMP/$ARCHIVE" -C "$TMP/unpack"
SRC="$TMP/unpack/nect-$TRIPLE$VARIANT/nect"
[ -f "$SRC" ] && [ -x "$SRC" ] || die "the archive did not contain a 'nect' executable"
VERSION=$("$SRC" --version) || die "the downloaded binary cannot run on this system"

mkdir -p "$INSTALL_DIR"
# Stage on the same filesystem, then rename: a failed copy preserves the old binary.
STAGED=$(mktemp "$INSTALL_DIR/.nect-install.XXXXXX")
trap 'rm -rf "$TMP"; rm -f "$STAGED"' EXIT
cp "$SRC" "$STAGED"
chmod 755 "$STAGED"
mv -f "$STAGED" "$INSTALL_DIR/nect"
say ""
say "Installed: $INSTALL_DIR/nect"
[ -n "$VERSION" ] && say "           $VERSION"

# --- PATH -------------------------------------------------------------------
#
# The point of doing this automatically is that the next command works without
# the user editing anything. An already-correct PATH is left alone, and the edit
# is marked so re-running the installer does not keep appending.

add_to_path() {
    marker="# added by the Nect installer"
    case ":$PATH:" in
        *":$INSTALL_DIR:"*)
            say ""
            say "PATH already contains $INSTALL_DIR."
            return 0
            ;;
    esac


    shell_name=$(basename "${SHELL:-sh}" 2>/dev/null || echo sh)
    case "$shell_name" in
        zsh) profile="$HOME/.zshrc" ;;
        bash) profile="$HOME/.bashrc" ;;
        fish) profile="$HOME/.config/fish/config.fish" ;;
        sh|dash|ksh) profile="$HOME/.profile" ;;
        *)
            say "Add $INSTALL_DIR to your $shell_name PATH manually."
            return 0
            ;;
    esac
    escaped_dir=$(printf '%s' "$INSTALL_DIR" | sed 's/[\\"$`]/\\&/g')
    if [ "$shell_name" = "fish" ]; then
        path_line="fish_add_path \"$escaped_dir\""
    else
        path_line="export PATH=\"$escaped_dir:\$PATH\""
    fi
    if grep -qxF "$path_line" "$profile" 2>/dev/null; then
        say ""
        say "$profile already has the Nect entry; leaving it as it is."
        return 0
    fi

    mkdir -p "$(dirname "$profile")"
    printf '\n%s\n%s\n' "$marker" "$path_line" >> "$profile"

    say ""
    say "Added $INSTALL_DIR to PATH in $profile."
    say "Open a new terminal (or run: . $profile) for it to take effect."
}

if [ "$NO_PATH" = "1" ]; then
    say ""
    say "--no-path was given, so PATH was left alone. Make sure $INSTALL_DIR is on it."
else
    add_to_path
fi

# --- verify -----------------------------------------------------------------

say ""
if command -v nect >/dev/null 2>&1; then
    say "Run 'nect --version' to verify, and 'nect run hello.nct' to try it."
else
    say "Run '$INSTALL_DIR/nect --version' to verify."
fi
