# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko
# port-checklist: reviewed 2026-10-16 by review-agent sha256=8ca35158129c6722e8b796a917d8060e00fde1cf4020b8997254a2df582961cd

"""json-roundtrip: parse, walk and rewrite a JSON document (see json-roundtrip.mi)."""

import json
import sys
import time


def walk(value):
    if isinstance(value, (dict, list)):
        nodes, total = 1, 0
        for item in value.values() if isinstance(value, dict) else value:
            inner_nodes, inner_total = walk(item)
            nodes += inner_nodes
            total += inner_total
        return nodes, total
    if isinstance(value, bool) or value is None or isinstance(value, str):
        return 1, 0
    return 1, value


def kernel(text):
    document = json.loads(text)
    nodes, total = walk(document)
    written = json.dumps(document, separators=(",", ":"))
    return f"{nodes} {total} {len(written)}"


def run_hot(n):
    expected = kernel(n)
    agree = True
    for _ in range(4):
        checksum = kernel(n)
        agree = agree and checksum == expected
    for _ in range(5):
        start = time.perf_counter_ns()
        checksum = kernel(n)
        elapsed = time.perf_counter_ns() - start
        print(f"hot_ns={elapsed}", file=sys.stderr)
        agree = agree and checksum == expected
    if not agree:
        print("checksum differs between iterations", file=sys.stderr)
        sys.exit(1)
    return expected


def main():
    with open(sys.argv[1]) as f:
        n = f.read()
    hot = len(sys.argv) > 2 and sys.argv[2] == "hot"
    print(run_hot(n) if hot else kernel(n))


if __name__ == "__main__":
    main()
