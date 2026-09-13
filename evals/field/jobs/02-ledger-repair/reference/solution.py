#!/usr/bin/env python3
# SPDX-License-Identifier: Apache-2.0
# Copyright (c) Viacheslav Shynkarenko

"""Reference solution for the ledger-repair job.

This is what the faulted ports become once every planted fault is repaired.
It is never copied into a subject's directory.
"""

import sys


def to_cents(amount):
    whole, fraction = amount.split(".")
    return int(whole) * 100 + int(fraction)


def format_cents(cents):
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}{cents // 100}.{cents % 100:02d}"


def main():
    lines = open(sys.argv[1], "rb").read().decode("utf-8").rstrip("\n").split("\n")
    n = int(lines[0].split(" ")[1])

    balances = {}
    for index in range(n):
        account, kind, amount = lines[1 + index].split(" ")
        if kind != "debit" and kind != "credit":
            print(f"ERROR line {index + 1}")
            return
        cents = to_cents(amount)
        balance = balances.get(account, 0)
        balances[account] = balance + cents if kind == "credit" else balance - cents

    for account, balance in sorted(balances.items(), key=lambda e: (-e[1], e[0])):
        print(f"{account} {format_cents(balance)}")
    print(f"TOTAL {format_cents(sum(balances.values()))}")


main()
