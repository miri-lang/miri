## Rule

A name exists in the type system but is not visible from the current scope due to visibility restrictions (e.g., private module members accessed from another module). The compiler enforces strict visibility rules to prevent unintended access to implementation details.

## Before

```miri
use system.io.println

fn main() i32:
  let x = system.io.print("hidden")
  0
```

## After

```miri
use system.io.{print, println}

fn main() i32:
  print("visible")
  0
```

## Reference

[Type Checker](../reference/types.md)
