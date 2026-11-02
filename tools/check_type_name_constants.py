#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
"""List every comparison against a `*_TYPE_NAME` constant in compiler code.

A type the compiler recognises by name is a type the compiler knows. That is
right for a language type (one the syntax produces, one with a `TypeKind` of
its own, one a kernel is handed) and wrong for a library type, which the
compiler must reach through a declaration instead. A constant hides the name
from the string-literal sweep, so this script lists the comparisons
themselves, each with the reason its constant may be compared, and fails on a
constant that has no reason here.

`src/ast/types.rs` declares the constants and the helpers that compare them,
and is not swept.
"""

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent

# Each constant a compiler stage may compare against, and why it is a
# language type rather than a library one.
ALLOWED = {
    "STRING_TYPE_NAME": "String is the primitive TypeKind::String; the name maps its spelling to the primitive",
    "OPTION_TYPE_NAME": "`T?` is TypeKind::Option; generic instantiations are normalised into it by name",
    "RESULT_TYPE_NAME": "the parser maps the written `Result` onto its normalised Custom form",
    "TUPLE_TYPE_NAME": "the type of tuple syntax; TypeKind::is_tuple recognises its Custom form",
    "SELF_TYPE_NAME": "`Self` is a keyword standing for the enclosing type, never a declared type",
    "LINEAR_TYPE_NAME": "`Linear<T>` is TypeKind::Linear, resolved from its spelling",
    "FUTURE_TYPE_NAME": "the parser maps the written `Future` onto TypeKind::Future",
    "ATOMIC_TYPE_NAME": "a GPU language type both backends lower to native atomics; its .mi declaration records the decision",
    "GPU_CONTEXT_TYPE_NAME": "the kernel context the compiler binds inside every `gpu fn`; it has no .mi declaration",
    "WARP_CONTEXT_TYPE_NAME": "the subgroup context the compiler binds inside kernels; it has no .mi declaration",
    "FRAME_INPUT_TYPE_NAME": "the per-frame input the compiler binds inside `gpu frame` bodies; it has no .mi declaration",
    "DIM3_TYPE_NAME": "the launch-dimension struct of the kernel context; it has no .mi declaration",
    "KERNEL_TYPE_NAME": "the opaque handle a `gpu fn` returns; it has no .mi declaration",
}

CONSTANT = re.compile(r"\b([A-Z0-9_]+_TYPE_NAME)\b")
COMPARISON = re.compile(
    r"(==|!=)\s*(?:[\w:]*::)?[A-Z0-9_]+_TYPE_NAME\b|\b[A-Z0-9_]+_TYPE_NAME\s*(==|!=)"
)
SKIPPED = ("src/ast/types.rs", "src/runtime/", "src/stdlib/")


def comparisons():
    for path in sorted((ROOT / "src").rglob("*.rs")):
        relative = path.relative_to(ROOT).as_posix()
        if relative.startswith(SKIPPED) or "/target/" in relative:
            continue
        for number, line in enumerate(path.read_text().splitlines(), start=1):
            code = line.split("//", 1)[0]
            if not COMPARISON.search(code):
                continue
            for constant in sorted(set(CONSTANT.findall(code))):
                yield relative, number, constant


def main():
    unlisted = []
    by_constant = {}
    for relative, number, constant in comparisons():
        by_constant.setdefault(constant, []).append(f"{relative}:{number}")
        if constant not in ALLOWED:
            unlisted.append(f"{relative}:{number}: {constant}")
    for constant in sorted(by_constant):
        reason = ALLOWED.get(constant, "NOT ALLOWED")
        print(f"  {constant} ({len(by_constant[constant])}): {reason}")
        for site in by_constant[constant]:
            print(f"    {site}")
    if unlisted:
        print("  ✗ a type name compared with no stated reason; reach the type through its")
        print("    declaration, or add the constant to ALLOWED with why it is a language type:")
        for site in unlisted:
            print(f"    {site}")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
