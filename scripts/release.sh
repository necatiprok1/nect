#!/bin/sh
# Release build script for Nect
# Builds binaries for all supported platforms and generates checksums.
# Usage: ./scripts/release.sh [version]

set -e

VERSION="${1:-$(git describe --tags --abbrev=0 2>/dev/null || echo '0.1.0')}"
echo "Building release v$VERSION..."

REPO_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO_DIR"

# Supported targets
TARGETS="x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu x86_64-apple-darwin aarch64-apple-darwin x86_64-pc-windows-msvc"

mkdir -p dist

for TARGET in $TARGETS; do
    echo "Building for $TARGET..."
    if rustup target list --installed 2>/dev/null | grep -q "$TARGET"; then
        cargo build --release --target "$TARGET" 2>/dev/null || {
            echo "  Warning: failed to build for $TARGET (target may not be installed)"
        }
        
        BIN="target/$TARGET/release/nect"
        if [ -f "$BIN" ]; then
            ARCHIVE_NAME="nect-$VERSION-$TARGET.tar.gz"
            tar -czf "dist/$ARCHIVE_NAME" -C "target/$TARGET/release" nect
            echo "  Created dist/$ARCHIVE_NAME"
        fi
    else
        echo "  Skipping $TARGET (not installed; run: rustup target add $TARGET)"
    fi
done

# Generate checksums
if [ "$(ls -A dist 2>/dev/null)" ]; then
    echo ""
    echo "Generating SHA-256 checksums..."
    cd dist
    sha256sum *.tar.gz > SHA256SUMS 2>/dev/null || shasum -a 256 *.tar.gz > SHA256SUMS
    echo "Checksums:"
    cat SHA256SUMS
    echo ""
    echo "Release artifacts in dist/"
fi
