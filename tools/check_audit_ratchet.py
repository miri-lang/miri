#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Keep the advisory counts of the mechanical audit from growing.

Three counts the audit lists but does not fail on are held against a
committed baseline: `unwrap()`/`expect()` calls in library code, broad `_ =>`
arms in the stages that define the compiler's own enums, and functions past
the eighty-line ceiling. A change may lower a count, and then lowers the
baseline with it; a change that raises one fails the audit.

    python3 tools/check_audit_ratchet.py            # compare with the baseline
    python3 tools/check_audit_ratchet.py --update   # write the current counts

The baseline lives in tools/audit_baseline.json. The function-length count
uses the same measure as the audit's own listing: the distance between one
`fn` line and the next, which overstates a function followed by free text,
so it is a ratchet, not a measurement.
"""

import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASELINE = ROOT / "tools" / "audit_baseline.json"

UNWRAP = re.compile(r"\.unwrap\(\)|\.expect\(")
WILDCARD_ARM = re.compile(r"_ =>")
FN_LINE = re.compile(r"^\s*(pub(\([a-z]+\))? )?(const )?(async )?(unsafe )?fn ")
FUNCTION_CEILING = 80
WILDCARD_SCOPES = ("src/mir", "src/type_checker", "src/codegen")


def library_files():
    """Every Rust file under src/ that is not a test."""
    for path in sorted((ROOT / "src").rglob("*.rs")):
        parts = path.relative_to(ROOT).parts
        if "target" in parts or "tests" in parts or path.name == "tests.rs":
            continue
        yield path


def lines_of(path):
    return path.read_text(encoding="utf-8").splitlines()


def count_unwraps():
    return sum(
        1
        for path in library_files()
        for line in lines_of(path)
        if UNWRAP.search(line) and "#[cfg(test)]" not in line
    )


def count_wildcard_arms():
    return sum(
        1
        for scope in WILDCARD_SCOPES
        for path in sorted((ROOT / scope).rglob("*.rs"))
        if "target" not in path.parts
        for line in lines_of(path)
        if WILDCARD_ARM.search(line)
    )


def oversized_functions(path):
    starts = [number for number, line in enumerate(lines_of(path), start=1) if FN_LINE.match(line)]
    ends = starts[1:] + [len(lines_of(path))]
    return sum(1 for start, end in zip(starts, ends) if end - start > FUNCTION_CEILING)


def count_oversized_functions():
    return sum(oversized_functions(path) for path in library_files())


def current_counts():
    return {
        "unwrap_expect_in_library_code": count_unwraps(),
        "wildcard_arms_in_mir_type_checker_codegen": count_wildcard_arms(),
        "functions_over_80_lines": count_oversized_functions(),
    }


def main(arguments):
    counts = current_counts()
    if "--update" in arguments:
        BASELINE.write_text(json.dumps(counts, indent=2) + "\n", encoding="utf-8")
        print(f"  baseline written: {counts}")
        return 0
    if not BASELINE.exists():
        print(f"  ✗ {BASELINE.relative_to(ROOT)} is missing; run with --update to create it")
        return 1
    baseline = json.loads(BASELINE.read_text(encoding="utf-8"))
    grew = []
    for name, count in counts.items():
        allowed = baseline.get(name)
        if allowed is None or count > allowed:
            grew.append(f"{name}: {count} (baseline {allowed})")
        elif count < allowed:
            print(f"  {name}: {count}, below the baseline of {allowed}; lower it with --update")
        else:
            print(f"  {name}: {count}, at the baseline")
    for item in grew:
        print(f"  ✗ {item}")
    if grew:
        print("  ✗ an audit count grew past tools/audit_baseline.json; fix the new hits, do not raise the baseline")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
