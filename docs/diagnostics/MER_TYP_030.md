## Rule

This code covers a family of argument count mismatches. When a generic type is instantiated with the wrong number of type arguments (e.g., `Map<int>` instead of `Map<int, string>`), or when a function call provides duplicate named arguments, this error is raised.

## Before

```miri
fn takes_two(a int, b int) int
    return a + b

fn main()
    let x = takes_two(1)
```

## After

```miri
fn takes_two(a int, b int) int
    return a + b

fn main()
    let x = takes_two(1, 2)
```

## Reference

[Type Checker](../reference/types.md)
