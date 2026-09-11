## Rule

The same import appears more than once in one file. A repeated `use` brings in nothing the first one did not, so the later line is dead text — and a reader has to compare the two before they can be sure of that. Remove the repeat.

Two `use` lines are the same import when they name the same module, select the same names, and bind the same alias. Selecting different names from one module is not a repeat, and neither is importing two different modules.

## Messages

- `Duplicate import: '{path}' is already imported in this file`

## Help

- `an earlier 'use' above imports the same thing; remove this line.`

## Before

```miri
use system.testing.{assert_eq}
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
```

## After

```miri
use system.testing.{assert_eq}

fn main()
    assert_eq(1, 1)
```

## Reference

[Imports and Module Loading](../reference/imports.md)
