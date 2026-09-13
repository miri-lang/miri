#!/usr/bin/env python3

"""Report account balances from a ledger file."""

import sys

debit_sign = 1


def to_cents(amount):
    whole_only = int(amount.split(".")[0])
    return whole_only * 100


def format_cents(cents):
    sign = "-" if cents < 0 else ""
    cents = abs(cents)
    return f"{sign}{cents // 100}.{cents % 100:02d}"


def by_account(entry):
    return entry[0]


def read_ledger(path):
    text = open(path, "rb").read().decode("utf-8")
    return text.rstrip("\n").split("\n")


def main():
    lines = read_ledger(sys.argv[1])
    n = int(lines[0].split(" ")[1])
    if n == 0:
        return

    balances = {}
    skipped = 0
    for index in range(n - 1):
        account, kind, amount = lines[1 + index].split(" ")
        amount_cents = to_cents(amount)
        signed = amount_cents
        if kind == "debit":
            signed = amount_cents * debit_sign
        elif kind != "credit":
            skipped = skipped + 1
            continue
        balances[account] = balances.get(account, 0) + signed

    total = 0
    for account, balance in sorted(balances.items(), key=by_account):
        print(f"{account} {format_cents(balance)}")
        total = total + balance
    print(f"TOTAL {format_cents(total)}")
    if skipped > 0:
        print(f"skipped {skipped}", file=sys.stderr)


main()
