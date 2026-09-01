## Rule

`miri fix --apply` was asked to write repairs and wrote none, because the file
carries errors that no repair answers. A repair reaches the command already
decided — the check that raised a diagnostic records it — so a diagnostic
without one is a report the compiler cannot act on for you.

Reporting this as success would tell a caller the file was repaired when it was
not, so the command exits non-zero and says which condition it hit.

A file carrying no errors is not this diagnostic. There was nothing to repair,
the apply had nothing to do, and doing nothing is the correct answer: the
command succeeds and exits zero. Warnings alone do not change that.

## Before

```sh
miri fix --apply main.mi
# error[MER_BLD_020]: No Repairs Applied
```

## After

```sh
# See what the compiler is reporting, and repair it by hand.
miri check main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
