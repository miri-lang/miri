#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Refuse internal planning vocabulary in the files the repository publishes.

The plan lives outside the repository, so a committed file that names one of
its identifiers — a task or milestone id, a design or feature code, a
benchmark round or claim number — reads as a reference nobody outside can
follow. Each such mention is replaced by a sentence saying what it means.

Every tracked text file is read. A pattern that also has a real meaning in a
few places (a variable called `r1`, a job's own fault table) is allowed there
by name, with the reason, so a new mention anywhere else fails.

    python3 tools/check_planning_vocabulary.py

Exits 1 and lists each mention when one is found.
"""

import fnmatch
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (name, pattern) — what each kind of planning identifier looks like.
PATTERNS = [
    ("task id", re.compile(r"\bT[0-9]+\.[0-9]+\b")),
    ("milestone id", re.compile(r"\bM[0-9]+\.[0-9]+\b")),
    ("milestone", re.compile(r"\bmilestones?\b", re.IGNORECASE)),
    ("design id", re.compile(r"\bDP[0-9]+(\.[0-9]+)?\b")),
    # F16/F32/F64/F128 are float type names, not feature codes.
    ("feature id", re.compile(r"\bF(?!16\b|32\b|64\b|128\b)[0-9]{1,3}\b")),
    ("benchmark round", re.compile(r"\br[0-9]+b?\b")),
    ("claim id", re.compile(r"\bC[1-9]\b")),
]

# (path glob, pattern name, why the mention is not a planning reference).
ALLOWED = [
    ("*.mi", "benchmark round", "`r0`, `r1` are variable names in Miri programs"),
    ("tests/integration/*", "benchmark round", "`r1`/`r2` are local variable names in test programs"),
    ("src/type_checker/compatibility.rs", "benchmark round", "`r1`/`r2` name two return types being compared"),
    ("evals/field/jobs/*/FAULTS.md", "feature id", "F1–F6 number the job's own seeded faults"),
    ("AGENTS.md", "milestone", "the agent instructions describe how the external plan is organised"),
    ("AGENTS.md", "milestone id", "the agent instructions describe how the external plan is organised"),
    (".claude/*", "milestone", "agent skills describe how the external plan is organised"),
    ("Makefile", "milestone", "the audit's own pattern for planning references"),
    ("tests/evals/field.rs", "milestone", "the gate's own list of words a subject's prompt must not carry"),
    ("tests/evals/field.rs", "task id", "the gate's own list of words a subject's prompt must not carry"),
    ("tests/evals/field.rs", "milestone id", "the gate's own list of words a subject's prompt must not carry"),
    ("tools/check_planning_vocabulary.py", "*", "this check's own patterns and allow-list"),
]

BINARY_SUFFIXES = {".png", ".gif", ".jpg", ".jpeg", ".ico", ".woff", ".woff2", ".pdf", ".a", ".o"}


def tracked_files():
    output = subprocess.run(
        ["git", "ls-files", "-z"], cwd=ROOT, check=True, capture_output=True
    ).stdout
    return [name for name in output.decode().split("\0") if name]


# A line that names an old identifier as what a current name used to be keeps
# the history readable; it says so with this word.
FORMER_NAME = "(formerly "


def allowed(path, pattern_name):
    return any(
        fnmatch.fnmatch(path, glob) and kind in ("*", pattern_name)
        for glob, kind, _ in ALLOWED
    )


def mentions(path):
    if Path(path).suffix.lower() in BINARY_SUFFIXES:
        return
    try:
        text = (ROOT / path).read_text(encoding="utf-8")
    except (UnicodeDecodeError, FileNotFoundError, IsADirectoryError):
        return
    for number, line in enumerate(text.splitlines(), start=1):
        for name, pattern in PATTERNS:
            match = pattern.search(line)
            if match and FORMER_NAME not in line and not allowed(path, name):
                yield f"{path}:{number}: {name} `{match.group(0)}`: {line.strip()[:120]}"


def main():
    found = [mention for path in tracked_files() for mention in mentions(path)]
    for mention in found:
        print(f"  {mention}")
    if found:
        print(f"  ✗ {len(found)} planning references in tracked files")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
