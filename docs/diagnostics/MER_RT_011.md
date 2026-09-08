## Rule

An index fell outside the collection it was applied to. The bounds check that guards every index caught it and ended the program rather than reading memory the collection does not own. Compare the index against the collection's length before using it, or reach for an accessor that answers with nothing when the index is out of range.

The same code reports a command-line argument asked for by a position that was not supplied.

## Before

```miri
fn at(a [int; 3], i int) int
    a[i]

fn main()
    let values = [1, 2, 3]
    println(f"{at(values, 7)}")
```

## After

```miri
fn at(a [int; 3], i int) int
    if i < a.length()
        return a[i]
    return 0

fn main()
    let values = [1, 2, 3]
    println(f"{at(values, 7)}")
```

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
