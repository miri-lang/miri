#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Run one slice of the `mod` integration test binary.

CI spreads the binary over several runners: `test_shard.py 2/3` runs the
second of three slices. The slices are cut from the binary's own test list,
so every test it holds runs in exactly one slice, a test added tomorrow
included, and the run fails if a slice comes up empty or the slices do not
add up to the list.

A slice is a set of whole modules rather than single tests: tests in one
module share fixtures and caches, and splitting them would make two runners
do that work. A test's cost depends mostly on its area (an `integration`
test compiles, links and runs a program; a `parser` test takes microseconds),
so each top-level area is dealt out on its own: its modules go largest first,
each to the slice holding the fewest of that area's tests so far.

Usage: test_shard.py INDEX/COUNT [libtest arguments...]
"""

import json
import subprocess
import sys


def mod_binary():
    """Build the integration binary and return its path."""
    build = subprocess.run(
        ["cargo", "test", "--test", "mod", "--no-run", "--message-format=json"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    for line in build.stdout.splitlines():
        message = json.loads(line)
        target = message.get("target", {})
        if message.get("executable") and target.get("name") == "mod":
            return message["executable"]
    sys.exit("test_shard: cargo built no `mod` test binary")


def test_names(binary):
    """Every test the binary holds, ignored ones included."""
    listing = subprocess.run(
        [binary, "--list", "--format", "terse"],
        check=True,
        stdout=subprocess.PIPE,
        text=True,
    )
    suffix = ": test"
    return sorted(
        line[: -len(suffix)] for line in listing.stdout.splitlines() if line.endswith(suffix)
    )


def module_of(name):
    """The module a test is declared in: its path without the last segment."""
    return name.rsplit("::", 1)[0] if "::" in name else ""


def slices(names, count):
    """Deal the tests out into `count` slices of whole modules."""
    areas = {}
    for name in names:
        area = name.split("::", 1)[0]
        areas.setdefault(area, {}).setdefault(module_of(name), []).append(name)
    dealt = [[] for _ in range(count)]
    for area in sorted(areas):
        modules = areas[area]
        in_area = [0] * count
        for module in sorted(modules, key=lambda m: (-len(modules[m]), m)):
            lightest = min(range(count), key=lambda i: (in_area[i], len(dealt[i]), i))
            in_area[lightest] += len(modules[module])
            dealt[lightest].extend(modules[module])
    return dealt


def parse_shard(argument):
    index, _, count = argument.partition("/")
    if not (index.isdigit() and count.isdigit()) or not 1 <= int(index) <= int(count):
        sys.exit(f"test_shard: expected INDEX/COUNT with 1 <= INDEX <= COUNT, got {argument!r}")
    return int(index), int(count)


def main():
    if len(sys.argv) < 2:
        sys.exit(__doc__)
    index, count = parse_shard(sys.argv[1])
    binary = mod_binary()
    names = test_names(binary)
    dealt = slices(names, count)
    if sorted(sum(dealt, [])) != names or any(not part for part in dealt):
        sys.exit("test_shard: the slices do not cover the test list exactly once")
    mine = dealt[index - 1]
    print(f"test_shard: slice {index}/{count} runs {len(mine)} of {len(names)} tests", flush=True)
    run = subprocess.run([binary, "--exact", *sys.argv[2:], *mine])
    sys.exit(run.returncode)


if __name__ == "__main__":
    main()
