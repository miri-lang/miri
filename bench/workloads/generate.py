# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Write the input file of a generated benchmark workload.

    python3 generate.py <workload> <parameter> <output path>

The bytes depend only on the workload and the parameter: every draw comes from
the MINSTD generator below, seeded the same way every time, so a manifest can
record the input's SHA-256 and the identity gate can regenerate it anywhere.
"""

import sys

MODULUS = 2147483647
LETTERS = "abcdefghijklmnopqrstuvwxyz"
MOTIF = "GGTATTTTAATTTATAGT"


class Minstd:
    """The Park-Miller "minimal standard" generator."""

    def __init__(self, seed):
        self.state = seed

    def next(self):
        self.state = self.state * 48271 % MODULUS
        return self.state

    def below(self, bound):
        return self.next() % bound


def word(rng):
    return "".join(LETTERS[rng.below(26)] for _ in range(3 + rng.below(8)))


def wordfreq(words):
    """`words` words drawn from a 5000-word vocabulary with a skewed
    frequency, twelve to a line."""
    rng = Minstd(1)
    vocabulary = [word(rng) for _ in range(5000)]
    lines = []
    for start in range(0, words, 12):
        count = min(12, words - start)
        picks = (rng.below(5000) * rng.below(5000) // 5000 for _ in range(count))
        lines.append(" ".join(vocabulary[p] for p in picks))
    return "\n".join(lines) + "\n"


def k_nucleotide(length):
    """A DNA sequence of `length` bases on one line, with the 18-base motif
    the workload counts written in roughly every 2000 bases."""
    rng = Minstd(2)
    bases = []
    while len(bases) < length:
        if rng.below(2000) == 0:
            bases.extend(MOTIF)
        else:
            bases.append("ACGT"[rng.below(4)])
    return "".join(bases[:length]) + "\n"


def json_records(records):
    """A JSON array of `records` objects: integers, short ASCII strings, a
    list and a nested object, so a parser meets every kind of value."""
    rng = Minstd(3)
    parts = []
    for i in range(records):
        tags = ",".join(f'"{word(rng)}"' for _ in range(1 + rng.below(4)))
        values = ",".join(str(rng.below(1000)) for _ in range(rng.below(6)))
        flag = "true" if rng.below(2) else "false"
        parts.append(
            f'{{"id":{i},"name":"{word(rng)}","score":{rng.below(100000)},'
            f'"active":{flag},"tags":[{tags}],'
            f'"position":{{"x":{rng.below(10000)},"y":{rng.below(10000)},"history":[{values}]}},'
            f'"parent":null}}'
        )
    return "[\n" + ",\n".join(parts) + "\n]\n"


def csv_rows(rows):
    """A header and `rows` records of a key from 50, a whole quantity and an
    amount with two decimals."""
    rng = Minstd(4)
    lines = ["region,quantity,amount"]
    for _ in range(rows):
        key = rng.below(50)
        cents = rng.below(100000)
        lines.append(f"region-{key:02d},{rng.below(1000)},{cents // 100}.{cents % 100:02d}")
    return "\n".join(lines) + "\n"


GENERATORS = {"wordfreq": wordfreq, "k-nucleotide": k_nucleotide, "json-roundtrip": json_records, "csv-aggregate": csv_rows}


def main():
    workload, parameter, path = sys.argv[1], int(sys.argv[2]), sys.argv[3]
    with open(path, "w", newline="\n") as f:
        f.write(GENERATORS[workload](parameter))


if __name__ == "__main__":
    main()
