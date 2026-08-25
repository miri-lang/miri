## Rule

A struct member declaration is missing its type. Struct fields must each be declared with both a name and a type. A field name without a following type annotation causes this error.

## Before

```miri
struct Point
    x
    y
```

## After

```miri
struct Point
    x int
    y int
```

## Reference

[Parser](../reference/parser.md)
