#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the keyed-totals job.

Python integers do not overflow, so the 64-bit range is enforced explicitly —
the same check every other language has to write, applied at the same point.
"""

import sys

MIN64 = -9223372036854775808
MAX64 = 9223372036854775807


def read_sections(path):
    lines = open(path, "rb").read().decode("utf-8").split("\n")
    records, queries, in_records = [], [], True
    for line in lines:
        if in_records and line == "":
            in_records = False
            continue
        if line == "":
            continue
        if in_records:
            key, value = line.split(" ", 1)
            records.append((key, int(value)))
        else:
            queries.append(line)
    return records, queries


def total_for(records, key):
    seen, total = False, 0
    for recorded, value in records:
        if recorded != key:
            continue
        seen = True
        total += value
        if total < MIN64 or total > MAX64:
            return "overflow"
    return str(total) if seen else "absent"


def main():
    records, queries = read_sections(sys.argv[1])
    for key in queries:
        print(f"{key}={total_for(records, key)}")


main()
