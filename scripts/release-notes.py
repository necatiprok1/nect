#!/usr/bin/env python3
"""Generate release notes from the commit log.

The notes are grouped by the kind of change a reader cares about, decided from
the conventional-commit prefix. Everything is derived from the log, so the notes
cannot describe work that was not done.

Usage:
    scripts/release-notes.py [since-tag]

    scripts/release-notes.py                 # since the most recent tag
    scripts/release-notes.py v0.1.0          # since a specific tag
    scripts/release-notes.py --unreleased    # every commit with no tag yet
    scripts/release-notes.py --format json   # machine-readable
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
from collections import OrderedDict

# Ordered because that is the order the sections appear in the output.
SECTIONS = OrderedDict(
    [
        ("breaking", ("Breaking changes", "These require a change to the program.")),
        ("feat", ("Features", "New capability.")),
        ("fix", ("Fixes", "A defect that was producing a wrong answer or an error.")),
        ("perf", ("Performance", "Measured, not assumed.")),
        ("security", ("Security", "Hardening, or a boundary made explicit.")),
        ("test", ("Tests", "Coverage, or a property now pinned.")),
        ("docs", ("Documentation", "Prose, and generated references.")),
        ("refactor", ("Internals", "No behaviour change intended.")),
        ("ci", ("Build and CI", "Pipelines and tooling.")),
        ("chore", ("Housekeeping", None)),
    ]
)

# `type(scope)!: subject` — the scope and the `!` are both optional.
CONVENTIONAL = re.compile(
    r"^(?P<type>[a-z]+)(?:\((?P<scope>[^)]*)\))?(?P<breaking>!)?:\s*(?P<subject>.+)$"
)


def git(*args: str) -> str:
    """Runs git and returns stdout, or an empty string if it fails."""
    result = subprocess.run(
        ["git", *args], capture_output=True, text=True, check=False
    )
    return result.stdout if result.returncode == 0 else ""


def latest_tag() -> str | None:
    tag = git("describe", "--tags", "--abbrev=0").strip()
    return tag or None


def commits_since(since: str | None) -> list[tuple[str, str]]:
    """Commits as `(sha, subject)`, oldest first."""
    revision_range = f"{since}..HEAD" if since else "HEAD"
    log = git("log", "--reverse", "--no-merges", "--format=%h%x1f%s", revision_range)
    commits = []
    for line in log.splitlines():
        if "\x1f" not in line:
            continue
        sha, subject = line.split("\x1f", 1)
        commits.append((sha, subject.strip()))
    return commits


def classify(subject: str) -> tuple[str | None, str, str | None, bool]:
    """Returns `(section, subject, scope, breaking)`.

    A commit that does not follow the convention is not dropped: it lands in
    `other`, because a note that silently loses a commit is worse than one that
    admits it could not categorise it.
    """
    match = CONVENTIONAL.match(subject)
    if not match:
        return ("other", subject, None, False)
    kind = match.group("type")
    section = "breaking" if match.group("breaking") else kind
    if section not in SECTIONS:
        section = "other"
    return (section, match.group("subject").strip(), match.group("scope"), bool(match.group("breaking")))


def build(commits: list[tuple[str, str]]) -> "OrderedDict[str, list[dict]]":
    grouped: OrderedDict[str, list[dict]] = OrderedDict()
    for sha, subject in commits:
        section, text, scope, breaking = classify(subject)
        entry = {"sha": sha, "subject": text, "scope": scope, "breaking": breaking}
        grouped.setdefault(section, []).append(entry)
    # Keep the declared section order, then anything unrecognised at the end.
    ordered: OrderedDict[str, list[dict]] = OrderedDict()
    for key in SECTIONS:
        if key in grouped:
            ordered[key] = grouped.pop(key)
    for key, entries in grouped.items():
        ordered[key] = entries
    return ordered


def render_markdown(version: str | None, since: str | None, grouped) -> str:
    total = sum(len(entries) for entries in grouped.values())
    out = [f"# Nect {version or 'unreleased'}\n"] if version else ["# Nect (unreleased)\n"]

    if total == 0:
        out.append("No commits since the previous release.\n")
        return "\n".join(out)

    out.append(
        f"{total} commit{'s' if total != 1 else ''}"
        + (f" since `{since}`" if since else "")
        + ".\n"
    )
    out.append(
        "Notes are generated from the commit log by `scripts/release-notes.py`.\n"
    )

    for section, entries in grouped.items():
        title, blurb = SECTIONS.get(section, ("Other changes", None))
        if section == "breaking":
            title, blurb = SECTIONS["breaking"]
        out.append(f"\n## {title}\n")
        if blurb:
            out.append(f"_{blurb}_\n")
        for entry in entries:
            scope = f"**{entry['scope']}**: " if entry["scope"] else ""
            out.append(f"- {scope}{entry['subject']} (`{entry['sha']}`)")

    return "\n".join(out) + "\n"


def main(argv: list[str]) -> int:
    fmt = "markdown"
    since: str | None = None
    for arg in argv:
        if arg == "--format":
            continue
        if arg == "json":
            fmt = "json"
        elif arg == "--unreleased":
            since = None
        elif arg.startswith("-"):
            print(f"unknown option: {arg}", file=sys.stderr)
            return 1
        else:
            since = arg

    if since is None and "--unreleased" not in argv:
        since = latest_tag()

    grouped = build(commits_since(since))
    if fmt == "json":
        print(json.dumps(grouped, indent=2))
        return 0

    version = since
    print(render_markdown(version, since, grouped), end="")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
