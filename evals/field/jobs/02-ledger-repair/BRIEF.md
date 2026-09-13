# Job — Repair the ledger report

The directory holds a program in {{LANGUAGE}} that reports account balances
from a ledger. It builds and runs, and it is wrong. Repair it.

The program's source is at `{{ENTRY_POINT}}`. Build and run it with
{{TOOLCHAIN}}.

## Contract the program must satisfy

The first command-line argument is the path of a text file:

```
ledger <n>
<account> <kind> <amount>      <- n lines follow the header
...
```

- `<kind>` is either `debit` or `credit`.
- `<amount>` is a decimal amount with exactly two fraction digits and no sign,
  such as `12.05`.
- An account's **balance** is the sum of its credits minus the sum of its
  debits, exact to the cent.

Write to standard output one line per account that appears in the ledger:

```
<account> <balance>
```

with the balance carrying two fraction digits, and a leading `-` when it is
below zero — `-3.07`, `0.00`, `41.50`. Order the lines by balance, descending;
break ties by account name, ascending, comparing bytes. Then write one final
line:

```
TOTAL <the sum of every balance>
```

in the same format.

Two cases the contract states outright:

- A ledger with no transactions produces exactly one line: `TOTAL 0.00`.
- A transaction line whose `<kind>` is neither `debit` nor `credit` produces
  exactly one line — `ERROR line <k>`, where `k` counts the transaction lines
  from 1 — and nothing else.

## Done

The job is finished when the program builds and satisfies the contract for any
ledger, not only the one you tried. There are no tests in the directory; the
contract above is the specification.
