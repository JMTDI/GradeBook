#!/usr/bin/env python3
"""Writes the notes for a GradeBook release as Markdown, from the git history and the installers.

    packaging/release-notes.py --version 1.2.0 --repo OWNER/REPO --installers DIR [--output FILE]

Run it in a git checkout that has the full history. The changes are the commits since the
previous release: the newest v<major>.<minor>.<patch> tag below this version that HEAD contains.
They are sorted into sections by the files they touch and by how the subject starts, so plain
messages like "Add a button that ..." make readable notes. Conventional prefixes such as
"feat:" and "fix:" are understood too, and "[skip notes]" in a subject (or a plain "Initial
commit") leaves a commit out.

It also writes SHA256SUMS.txt next to the installers (to be attached to the release as well)
and lists the installers, with their sizes, in a table of download links.
"""
from __future__ import annotations

import argparse
import datetime
import hashlib
import html
import os
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path
from urllib.parse import quote

# key, emoji, heading, and the words used for a count of them in the summary line.
SECTIONS = [
    ("new", "✨", "What's new", "new feature", "new features"),
    ("improved", "🎨", "Improvements", "improvement", "improvements"),
    ("fixed", "🐛", "Fixes", "fix", "fixes"),
    ("docs", "📚", "Documentation", "docs change", "docs changes"),
    ("build", "🏗️", "Build and packaging", "build change", "build changes"),
    ("tests", "✅", "Tests", "test change", "test changes"),
    ("other", "🔧", "Other changes", "other change", "other changes"),
]

# "feat(ui): add a thing", "fix: a bug", ...
CONVENTIONAL = {
    "feat": "new", "feature": "new",
    "fix": "fixed", "bugfix": "fixed", "hotfix": "fixed",
    "perf": "improved", "refactor": "improved", "style": "improved", "improve": "improved",
    "docs": "docs", "doc": "docs",
    "build": "build", "ci": "build", "chore": "build", "deps": "build",
    "test": "tests", "tests": "tests",
}
CONVENTIONAL_RE = re.compile(r"^(?P<type>[A-Za-z]+)(?:\([^)]*\))?!?:\s+(?P<rest>\S.*)$")

# The first word of a plain subject ("Add a button ...").
VERBS = {
    "new": {"add", "introduce", "support", "implement", "create", "enable", "allow"},
    "fixed": {"fix", "correct", "prevent", "resolve", "repair", "avoid"},
    "improved": {
        "improve", "make", "refresh", "polish", "update", "rework", "change", "list", "simplify",
        "speed", "tweak", "show", "use", "move", "rename", "refine", "clean", "tidy", "reduce",
        "increase", "extend",
    },
}

# A commit that only touches these is sorted by that alone, whatever its subject says.
FILE_SECTIONS = [
    ("docs", ("README.md", "docs/", "LICENSE")),
    ("build", (".github/", "packaging/", ".cargo/", "Cargo.toml", "Cargo.lock", "build.rs", "rustfmt.toml", ".gitignore")),
    ("tests", ("tests/",)),
]

SKIP = re.compile(r"\[skip (notes|changelog)\]|^initial commit$", re.IGNORECASE)

# Installer file name endings, in the order of the download table.
DOWNLOADS = [
    (".exe", "🪟", "**Windows** 10/11 (64-bit)"),
    (".dmg", "🍎", "**macOS** 11+ (Apple Silicon and Intel)"),
    (".deb", "🐧", "**Debian, Ubuntu** and derivatives"),
    (".AppImage", "🐧", "**Any other Linux** (64-bit)"),
]
CHECKSUM_FILE = "SHA256SUMS.txt"

SLINT_BADGE = """\
<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-light.png">
  <a href="https://slint.dev"><img src="https://raw.githubusercontent.com/slint-ui/slint/master/logo/MadeWithSlint-logo-dark.png" alt="Made with Slint" width="160"></a>
</picture>

</div>"""


@dataclass
class Commit:
    sha: str
    short: str
    subject: str
    files: list[str]


def git(*args: str) -> str:
    result = subprocess.run(
        ["git", "-c", "core.quotepath=false", *args], capture_output=True, text=True, encoding="utf-8"
    )
    if result.returncode != 0:
        sys.exit(f"release-notes: git {' '.join(args)} failed: {result.stderr.strip()}")
    return result.stdout


def parse_version(text: str) -> tuple[int, int, int] | None:
    match = re.fullmatch(r"v?(\d+)\.(\d+)\.(\d+)", text)
    return (int(match[1]), int(match[2]), int(match[3])) if match else None


def previous_tag(version: str) -> str | None:
    """The newest release tag below `version` that the current commit contains."""
    current = parse_version(version)
    best: tuple[tuple[int, int, int], str] | None = None
    for tag in git("tag", "--list", "v[0-9]*").split():
        key = parse_version(tag)
        if key is None or key >= current:
            continue
        reachable = subprocess.run(["git", "merge-base", "--is-ancestor", tag, "HEAD"]).returncode == 0
        if reachable and (best is None or key > best[0]):
            best = (key, tag)
    return best[1] if best else None


def commits_since(tag: str | None) -> list[Commit]:
    revision = f"{tag}..HEAD" if tag else "HEAD"
    log = git("log", "--no-merges", "--name-only", "--format=%x01%H%x02%h%x02%s", revision)
    found = []
    for block in log.split("\x01")[1:]:
        header, _, files = block.partition("\n")
        sha, short, subject = header.split("\x02", 2)
        found.append(Commit(sha, short, subject.strip(), [f for f in files.splitlines() if f.strip()]))
    return found


def tidy(text: str) -> str:
    text = text.strip().rstrip(".")
    return html.escape(text[:1].upper() + text[1:], quote=False)


def classify(commit: Commit) -> tuple[str, str]:
    """The section a commit belongs in, and the text to show for it."""
    conventional = CONVENTIONAL_RE.match(commit.subject)
    if conventional and conventional["type"].lower() in CONVENTIONAL:
        return CONVENTIONAL[conventional["type"].lower()], tidy(conventional["rest"])
    text = tidy(commit.subject)
    if commit.files:
        for key, prefixes in FILE_SECTIONS:
            if all(name.startswith(prefixes) for name in commit.files):
                return key, text
    first_word = re.match(r"[A-Za-z]+", commit.subject)
    word = first_word[0].lower() if first_word else ""
    for key, verbs in VERBS.items():
        if word in verbs:
            return key, text
    return "other", text


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as handle:
        for chunk in iter(lambda: handle.read(1 << 20), b""):
            digest.update(chunk)
    return digest.hexdigest()


def human_size(size: int) -> str:
    return f"{size / 1_000_000:.1f} MB" if size >= 100_000 else f"{size / 1_000:.0f} kB"


def installers_in(folder: Path) -> list[Path]:
    if not folder.is_dir():
        return []
    return sorted(p for p in folder.iterdir() if p.is_file() and p.name != CHECKSUM_FILE)


def write_checksums(files: list[Path], folder: Path) -> list[str]:
    lines = [f"{sha256(path)}  {path.name}" for path in files]
    (folder / CHECKSUM_FILE).write_text("\n".join(lines) + "\n", encoding="utf-8")
    return lines


def render(
    *, version: str, repo: str, sha: str, day: datetime.date, previous: str | None,
    sections: dict[str, list[tuple[str, str, str]]], files: list[Path], checksums: list[str],
) -> str:
    repo_url = f"https://github.com/{repo}"
    tag = f"v{version}"
    total = sum(len(items) for items in sections.values())
    when = f"{day.day} {day:%B %Y}"
    out: list[str] = []

    if previous:
        since = f"[{previous}]({repo_url}/releases/tag/{previous})"
        out.append(f"> 🗓️ **{when}** · {total} {'change' if total == 1 else 'changes'} since {since}")
    else:
        out.append(f"> 🎉 **The first release of GradeBook** · {when}")
    counts = [
        f"{emoji} **{len(sections[key])}** {one if len(sections[key]) == 1 else many}"
        for key, emoji, _heading, one, many in SECTIONS
        if sections.get(key)
    ]
    if counts:
        out += ["", " · ".join(counts)]

    if files:
        out += ["", "### ⬇️ Downloads", "", "| | System | Download | Size |", "|---|---|---|---|"]
        for suffix, icon, label in DOWNLOADS:
            for path in files:
                if path.name.endswith(suffix):
                    link = f"[`{path.name}`]({repo_url}/releases/download/{tag}/{quote(path.name)})"
                    out.append(f"| {icon} | {label} | {link} | {human_size(path.stat().st_size)} |")
        out += [
            "",
            "> [!NOTE]",
            "> The installers aren't code-signed, so Windows and macOS show a warning the first time "
            f"you open GradeBook. [How to open it anyway]({repo_url}/blob/{sha}/README.md#install).",
        ]

    if total == 0:
        out += ["", "_There are no code changes since the last release._"]
    for key, emoji, heading, _one, _many in SECTIONS:
        items = sections.get(key)
        if items:
            out += ["", f"### {emoji} {heading}", ""]
            out += [f"- {text} ([`{short}`]({repo_url}/commit/{full}))" for text, short, full in items]

    if checksums:
        out += [
            "", "<details>", "<summary>🔐 SHA-256 checksums</summary>", "", "```text", *checksums, "```", "",
            f"`{CHECKSUM_FILE}` is attached to this release. Check your downloads with "
            f"`sha256sum -c {CHECKSUM_FILE}` (on macOS: `shasum -a 256 -c {CHECKSUM_FILE}`).",
            "", "</details>",
        ]

    if previous:
        out += ["", f"**Full changelog:** [`{previous}...{tag}`]({repo_url}/compare/{previous}...{tag})"]
    else:
        out += ["", f"**Full changelog:** [all commits]({repo_url}/commits/{tag})"]
    out += ["", SLINT_BADGE]
    return "\n".join(out) + "\n"


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("--version", required=True, help="the version being released, like 1.2.0")
    parser.add_argument("--repo", default=os.environ.get("GITHUB_REPOSITORY"), help="OWNER/REPO (default: $GITHUB_REPOSITORY)")
    parser.add_argument("--installers", type=Path, default=Path("installers"), help="folder with the built installers")
    parser.add_argument("--output", type=Path, help="write the notes here instead of to the terminal")
    parser.add_argument("--previous-tag", help="compare with this tag instead of the newest earlier release")
    parser.add_argument("--date", type=datetime.date.fromisoformat, help="release date, YYYY-MM-DD (default: today, UTC)")
    args = parser.parse_args()

    if parse_version(args.version) is None or args.version.startswith("v"):
        sys.exit(f"release-notes: '{args.version}' is not a version like 1.2.0")
    if not args.repo:
        sys.exit("release-notes: pass --repo OWNER/REPO (or set GITHUB_REPOSITORY)")

    previous = args.previous_tag or previous_tag(args.version)
    sections: dict[str, list[tuple[str, str, str]]] = {}
    for commit in commits_since(previous):
        if SKIP.search(commit.subject):
            continue
        key, text = classify(commit)
        sections.setdefault(key, []).append((text, commit.short, commit.sha))

    files = installers_in(args.installers)
    checksums = write_checksums(files, args.installers) if files else []
    if not files:
        print(f"release-notes: no installers found in {args.installers}", file=sys.stderr)

    notes = render(
        version=args.version,
        repo=args.repo,
        sha=git("rev-parse", "HEAD").strip(),
        day=args.date or datetime.datetime.now(datetime.timezone.utc).date(),
        previous=previous,
        sections=sections,
        files=files,
        checksums=checksums,
    )
    if args.output:
        args.output.write_text(notes, encoding="utf-8")
        total = sum(len(items) for items in sections.values())
        print(
            f"release-notes: wrote {args.output} (changes since {previous or 'the beginning'}: {total}, "
            f"installers: {len(files)})",
            file=sys.stderr,
        )
    else:
        sys.stdout.write(notes)


if __name__ == "__main__":
    main()
