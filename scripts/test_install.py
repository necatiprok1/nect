#!/usr/bin/env python3
"""Offline installer tests: python3 scripts/test_install.py (no Rust required)."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest


INSTALLER = Path(__file__).with_name("install.sh").resolve()


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory(prefix="nect-installer-test-")
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.home = self.root / "home"
        self.home.mkdir()
        self.bin = self.root / "tools"
        self.bin.mkdir()
        self.assets = self.root / "assets"
        self.assets.mkdir()
        self.install = self.home / "install with spaces"
        self.env = dict(os.environ, HOME=str(self.home), SHELL="/bin/zsh",
                        PATH=str(self.bin) + os.pathsep + os.environ["PATH"],
                        INSTALL_DIR=str(self.install), NECT_FULL="0", NECT_NO_PATH="0",
                        NECT_VERSION="", MOCK_ASSETS=str(self.assets),
                        MOCK_URLS=str(self.root / "urls"), MOCK_OS="Darwin", MOCK_ARCH="arm64")
        self.tool("uname", '#!/bin/sh\ncase "$1" in -s) echo "$MOCK_OS";; -m) echo "$MOCK_ARCH";; esac\n')
        # Exercise the real script, archive tools and checksum verifier, but never the network.
        self.tool("curl", '''#!/bin/sh
url=""
out=""
while [ "$#" -gt 0 ]; do
    case "$1" in
        -o) out="$2"; shift ;;
        https://*) url="$1" ;;
    esac
    shift
done
printf '%s\\n' "$url" >> "$MOCK_URLS"
cp "$MOCK_ASSETS/${url##*/}" "$out"
''')
        self.make_archive()

    def tool(self, name, contents):
        path = self.bin / name
        path.write_text(contents)
        path.chmod(0o755)

    def make_archive(self, triple="aarch64-apple-darwin", full=False, binary=True, runnable=True):
        name = f"nect-{triple}{'-full' if full else ''}"
        archive = self.assets / f"{name}.tar.gz"
        with tarfile.open(archive, "w:gz") as tar:
            if binary:
                contents = b'#!/bin/sh\nprintf "nect test\\n"\n' if runnable else b'#!/bin/sh\nexit 1\n'
                member = tarfile.TarInfo(f"{name}/nect")
                member.size = len(contents)
                member.mode = 0o755
                tar.addfile(member, io.BytesIO(contents))
        digest = hashlib.sha256(archive.read_bytes()).hexdigest()
        (self.assets / "SHA256SUMS").write_text(f"{digest}  {archive.name}\n")

    def run_installer(self, *args, piped=False):
        command = ["sh", "-s", "--", *args] if piped else ["sh", str(INSTALLER), *args]
        return subprocess.run(command, input=INSTALLER.read_text() if piped else None,
                              env=self.env, text=True, capture_output=True, timeout=15)

    def preserve_existing(self):
        self.install.mkdir()
        (self.install / "nect").write_text("old installation")

    def assert_refused(self, result):
        self.assertNotEqual(result.returncode, 0, result.stdout)
        self.assertEqual((self.install / "nect").read_text(), "old installation")
        self.assertFalse((self.home / ".zshrc").exists())

    def test_success_creates_profile_and_is_idempotent(self):
        result = self.run_installer(piped=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Checksum verified", result.stdout)
        installed = self.install / "nect"
        self.assertTrue(os.access(installed, os.X_OK))
        profile = self.home / ".zshrc"
        first = profile.read_text()
        self.assertIn(str(self.install), first)
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(first, profile.read_text())
        self.assertIn("github.com/necatiprok1/nect/releases/latest/download/", (self.root / "urls").read_text())

    def test_no_path(self):
        result = self.run_installer("--no-path")
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse((self.home / ".zshrc").exists())

    def test_full_and_version_tags(self):
        self.make_archive(full=True)
        for tag in ("0.1.0", "v0.1.0"):
            with self.subTest(tag=tag):
                result = self.run_installer("--full", "--version", tag)
                self.assertEqual(result.returncode, 0, result.stderr)
        urls = (self.root / "urls").read_text()
        self.assertIn("/download/v0.1.0/nect-aarch64-apple-darwin-full.tar.gz", urls)
        self.assertNotIn("/download/vv", urls)

    def test_missing_manifest_preserves_existing(self):
        self.preserve_existing()
        (self.assets / "SHA256SUMS").unlink()
        result = self.run_installer()
        self.assert_refused(result)
        self.assertIn("refusing an unverified", result.stderr)

    def test_missing_manifest_entry_preserves_existing(self):
        self.preserve_existing()
        (self.assets / "SHA256SUMS").write_text("0" * 64 + "  other.tar.gz\n")
        self.assert_refused(self.run_installer())

    def test_corrupt_download_preserves_existing(self):
        self.preserve_existing()
        archive = next(self.assets.glob("*.tar.gz"))
        with archive.open("ab") as file:
            file.write(b"tampered")
        result = self.run_installer()
        self.assert_refused(result)
        self.assertIn("checksum mismatch", result.stderr)

    def test_missing_binary_preserves_existing(self):
        self.preserve_existing()
        self.make_archive(binary=False)
        self.assert_refused(self.run_installer())

    def test_unrunnable_binary_preserves_existing(self):
        self.preserve_existing()
        self.make_archive(runnable=False)
        result = self.run_installer()
        self.assert_refused(result)
        self.assertIn("cannot run", result.stderr)

    def test_supported_targets(self):
        for system, arch, triple in (
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Linux", "x86_64", "x86_64-unknown-linux-gnu"),
            ("Linux", "aarch64", "aarch64-unknown-linux-gnu"),
        ):
            with self.subTest(system=system, arch=arch):
                self.env.update(MOCK_OS=system, MOCK_ARCH=arch)
                self.make_archive(triple=triple)
                result = self.run_installer("--no-path")
                self.assertEqual(result.returncode, 0, result.stderr)

    def test_unsupported_architecture_does_not_download(self):
        self.env["MOCK_ARCH"] = "riscv64"
        result = self.run_installer()
        self.assertNotEqual(result.returncode, 0)
        self.assertFalse((self.root / "urls").exists())

    def test_help_works_when_piped(self):
        result = self.run_installer("--help", piped=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("Usage:", result.stdout)
        self.assertFalse((self.root / "urls").exists())

    def test_profile_quotes_custom_path_safely(self):
        self.install = self.home / 'bin $literal `literal` "quote"'
        self.env["INSTALL_DIR"] = str(self.install)
        result = self.run_installer()
        self.assertEqual(result.returncode, 0, result.stderr)
        profile = self.home / ".zshrc"
        result = subprocess.run(["sh", "-c", '. "$HOME/.zshrc"; command -v nect'],
                                env=self.env, text=True, capture_output=True, timeout=5)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout.strip(), str(self.install / "nect"))

    def test_fish_and_sh_profiles_created(self):
        for shell, profile in (("fish", ".config/fish/config.fish"), ("sh", ".profile")):
            with self.subTest(shell=shell):
                self.env["SHELL"] = "/bin/" + shell
                result = self.run_installer()
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertTrue((self.home / profile).is_file())


if __name__ == "__main__":
    unittest.main()
