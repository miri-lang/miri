## Rule

An enum variant was referenced that does not exist in the enum's definition, or was used with an incorrect structure (wrong number or types of fields). This code covers multiple family members: undefined variants, constructor arity mismatches, and guard pattern incompatibilities.

## Before

```miri
enum Status:
  Success
  Error

fn main() i32:
  let s = Status.Pending
  0
```

## After

```miri
enum Status:
  Success
  Error
  Pending

fn main() i32:
  let s = Status.Pending
  0
```

## Reference

[Type Checker](../reference/types.md)
