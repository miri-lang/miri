## Rule

An operation was attempted with an invalid operand at runtime. The operand does not satisfy the preconditions of the operation (for example, a negative value where only non-negative values are allowed). Validate operands before calling operations with restrictions.

## Before

```miri
let x = -5
let result = sqrt(x)
```

## After

```miri
let x = 5.0
let result = sqrt(x)
println(result)
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
