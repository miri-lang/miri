#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the word-frequency job.

The hidden-test expectations are generated from this file, never typed by hand:
an expectation a person wrote is a second implementation nothing checks.
"""

import re
import sys


def main():
    text = open(sys.argv[1], "rb").read().decode("utf-8", "replace")
    counts = {}
    for word in re.findall(r"[A-Za-z0-9]+", text):
        lowered = word.lower()
        counts[lowered] = counts.get(lowered, 0) + 1
    ranked = sorted(counts.items(), key=lambda pair: (-pair[1], pair[0]))
    for word, count in ranked[:10]:
        print(f"{word} {count}")


main()
