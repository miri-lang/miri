#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Refuse an import that crosses a layer boundary the principles close.

Each rule names a layer, the import that layer may not make, and why. A file
that makes the import today is listed with the reason it is still allowed,
so the rule catches every new file while the listed ones are repaired; an
entry is removed when its file no longer needs it, never added without a
reason. Only code is read: a comment or a doc line naming the import is not
an import.

The script also lists every `Placement exception:` comment in the compiler,
which is how a duplicate kept for performance is recorded (PRINCIPLES.md,
optimization placement). The list is advisory; the rules fail the audit.

    python3 tools/check_layering.py

Exits 1 and lists each violation when one is found.
"""

import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# (rule name, directory prefix, forbidden import pattern, why it is forbidden).
RULES = [
    (
        "codegen reads MIR, not AST expressions",
        "src/codegen/",
        re.compile(r"crate::ast::(expression|factory)\b"),
        "a backend consumes resolved types and plans from MIR; a type argument "
        "still spelled as an AST expression is a MIR gap, not a backend concern",
    ),
    (
        "a verifier shares no analysis with the pass it checks",
        "src/mir/verify",
        re.compile(r"\bmir::optimization::|\bsuper::optimization::"),
        "a verifier is a second derivation; one that calls the pass's own "
        "analysis passes whenever the analysis is wrong",
    ),
    (
        "the type checker learns nothing from a runtime symbol name",
        "src/type_checker/",
        re.compile(r"\bruntime_fns\b"),
        "what an intrinsic does to its arguments is stated on its declaration, "
        "which the type checker already reads",
    ),
]

# (path, rule name, why the import is still there). Known debt: each entry is
# removed by the change that repairs the file, and no entry is added without
# the reason the repair cannot land first.
ALLOWED = [
    # Codegen unwraps type arguments stored as AST expressions; gone once MIR
    # carries resolved type arguments.
    ("src/codegen/cranelift/layout.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/predicates.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/rc.rs", "codegen reads MIR, not AST expressions", "unwraps and rebuilds AST type arguments"),
    ("src/codegen/cranelift/structural_elements.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/translate_rvalue.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/translate_statement.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/translator.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/types.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/cranelift/value_abi.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/wgsl/emitter.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    ("src/codegen/wgsl/types.rs", "codegen reads MIR, not AST expressions", "unwraps AST type arguments"),
    # The borrow verifier reuses the borrow inference; gone once it has its
    # own conservative derivation. The in-bounds verifier reads one
    # specification constant (whether integer arithmetic traps on overflow)
    # that belongs in a specification module, not in the pass.
    ("src/mir/verify/local_borrows.rs", "a verifier shares no analysis with the pass it checks", "reuses the borrow inference's reach"),
    ("src/mir/verify/in_bounds.rs", "a verifier shares no analysis with the pass it checks", "reads the overflow-trap specification constant from the pass"),
    # The type checker reads effect facts keyed by symbol name; gone once the
    # facts are attributes on the runtime declarations.
    ("src/type_checker/receiver_writes.rs", "the type checker learns nothing from a runtime symbol name", "asks which argument positions an intrinsic only reads"),
    ("src/type_checker/instantiation_requirements.rs", "the type checker learns nothing from a runtime symbol name", "asks which intrinsics order or match elements"),
    ("src/type_checker/statements/runtime_closures.rs", "the type checker learns nothing from a runtime symbol name", "asks the closure contract of an intrinsic"),
    ("src/type_checker/reference_cycles.rs", "the type checker learns nothing from a runtime symbol name", "asks which collections copy on write"),
]

PLACEMENT_EXCEPTION = "Placement exception:"


def rust_files(prefix):
    return sorted(
        path
        for path in (ROOT / prefix).rglob("*.rs")
        if "target" not in path.parts
    ) if (ROOT / prefix).is_dir() else sorted(ROOT.glob(prefix + "*.rs"))


def code_lines(path):
    """The lines of a Rust file with comments removed, numbered from 1."""
    try:
        text = path.read_text(encoding="utf-8")
    except UnicodeDecodeError:
        return
    for number, line in enumerate(text.splitlines(), start=1):
        stripped = line.strip()
        if stripped.startswith("//"):
            continue
        yield number, line.split("//", 1)[0]


def is_allowed(relative, rule_name):
    return any(path == relative and rule == rule_name for path, rule, _ in ALLOWED)


def violations():
    for rule_name, prefix, pattern, _ in RULES:
        for path in rust_files(prefix):
            relative = path.relative_to(ROOT).as_posix()
            if not relative.startswith(prefix):
                continue
            for number, code in code_lines(path):
                if pattern.search(code) and not is_allowed(relative, rule_name):
                    yield f"{relative}:{number}: {rule_name}: {code.strip()[:100]}"


def unused_allowances():
    """Allow-list entries whose file no longer makes the import."""
    for relative, rule_name, _ in ALLOWED:
        pattern = next(p for name, _, p, _ in RULES if name == rule_name)
        path = ROOT / relative
        if not path.exists() or not any(pattern.search(code) for _, code in code_lines(path)):
            yield f"{relative}: allow-list entry for '{rule_name}' is no longer needed; remove it"


def placement_exceptions():
    for path in rust_files("src/"):
        for number, line in enumerate(path.read_text(encoding="utf-8").splitlines(), start=1):
            if PLACEMENT_EXCEPTION in line:
                yield f"{path.relative_to(ROOT).as_posix()}:{number}: {line.strip()[:110]}"


def main():
    found = list(violations()) + list(unused_allowances())
    for item in found:
        print(f"  {item}")
    exceptions = list(placement_exceptions())
    print(f"  {len(exceptions)} placement exception(s) recorded in src/:")
    for item in exceptions:
        print(f"    {item}")
    if found:
        print(f"  ✗ {len(found)} layer violation(s); see the rules in tools/check_layering.py")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
