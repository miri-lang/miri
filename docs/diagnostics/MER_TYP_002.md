## Rule

The compiler enforces type compatibility at assignment, return, and function-call sites. When the type of a value does not match the declared or inferred type expected at that location, a type mismatch error is raised.

The same code covers operands an operator cannot combine. `+` never converts between types, so joining text to a number is a mismatch rather than a conversion; an f-string is what renders a value into text. An optional is likewise not a number: it must be given a default with `??` or matched on before it takes part in arithmetic.

## Before

```miri
var n = 5
let msg = "n=" + n
println(f"{msg}")
```

## After

```miri
var n = 5
let msg = f"n={n}"
println(f"{msg}")
```

## Reference

[Type Checker](../reference/types.md)
