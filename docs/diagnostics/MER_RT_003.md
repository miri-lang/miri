## Rule

An arithmetic operation caused an integer overflow at runtime. The result exceeded the maximum value that can be represented in the integer type. Use a wider type or add range checks before operations that could overflow.

## Before

```miri
let x = i32(2147483647)
let y = 1
let result = x + y
```

## After

```miri
let x = i64(2147483647)
let y = i64(1)
let result = x + y
println(result)
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
