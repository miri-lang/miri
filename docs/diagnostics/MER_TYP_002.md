## Rule

The compiler enforces type compatibility at assignment, return, and function-call sites. When the type of a value does not match the declared or inferred type expected at that location, a type mismatch error is raised.

The same code covers operands an operator cannot combine. `+` never converts between types, so joining text to a number is a mismatch rather than a conversion; an f-string is what renders a value into text. An optional is likewise not a number: it must be given a default with `??` or matched on before it takes part in arithmetic.

## Messages

- `Type mismatch: cannot add {left} and {right} (both must be the same type)`
- `Type mismatch: cannot {op} a float to an integer`
- `Type mismatch: cannot multiply {left} by {right} (right operand must be an integer)`
- `Type mismatch: {left} and {right} are not compatible for arithmetic operation`
- `Invalid types for arithmetic operation: {left} and {right}`
- `Type '{type}' is not numeric: {function} only accepts int or float arguments`
- `Type mismatch in {context}: expected {expected}, got {actual}`
- `Type mismatch for argument '{name}': expected {expected}, got {actual}`
- `Type mismatch for field '{name}': expected {expected}, got {actual}`
- `Too many positional arguments: expected {expected}, got {actual}`
- `Member property must be an identifier`
- `Type mismatch for variable '{name}': expected {expected}, got {actual}`
- `If condition must be a boolean, got {type}`
- `While condition must be a boolean, got {type}`
- `Type mismatch for loop variable '{name}': expected Int, got {actual}`
- `Type mismatch for loop variable '{name}': expected {expected}, got {actual}`
- `Invalid return type: expected {expected}, got {actual}`
- `Match branch types mismatch: expected {expected}, got {actual}`
- `Conditional condition must be a boolean, got {type}`
- `Conditional branches must have the same type: expected {expected}, got {actual}`
- `Type mismatch for default value: expected {expected}, got {actual}`
- `Type mismatch: guard must be boolean, got {type}`
- `Type {type} is not iterable`

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
