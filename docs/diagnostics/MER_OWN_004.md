## Rule

An expression value of a type marked `@must_use` was discarded. The type author has declared that values of this type should never be ignored, usually because ignoring the value represents a logical error or a performance mistake.

## Before

```miri
@must_use
enum Result
    Ok(i32)
    Err(String)

fn main()
    Result.Ok(42)
```

## After

```miri
@must_use
enum Result
    Ok(i32)
    Err(String)

fn main()
    let r = Result.Ok(42)
    println(f"{r}")
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
