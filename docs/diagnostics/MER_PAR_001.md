## Rule

The parser encountered a token that was not expected at this position. A specific token was required to parse the current construct (e.g. an identifier after `fn`, a colon after a type annotation), but a different token was found instead. This is a family code covering dozens of specific parse contexts where the wrong token appears.

Some constructs that fail here are unambiguous syntax from another language.
Where the failing token names one, the diagnostic carries help naming the Miri
spelling, and a repair when the rewrite is textual.

| Written | Miri | Repair |
|---|---|---|
| `let x: int = 5` | `let x int = 5` | `colon-annotation` |
| `fn main() -> int` | `fn main() int` | `arrow-return-type` |
| `let mut x = 5` | `var x = 5` | `let-mut-to-var` |
| `println!("hi")` | `println("hi")` | `println-bang` |
| `return null` | `return None` | `null-to-none` |
| `if x { … }` | `if x:` or an indented body | — |
| `elif` | `else if` | — |
| `impl Foo` | methods inside the class body | — |
| `for (k, v) in m` | `for k in m`, then `m.get(k)` | — |
| `let (a, b) = pair()` | one name per binding | — |

The last five change the shape of the code rather than its text, so they carry
help but no repair: writing them would mean inventing code the author has not
written.

## Messages

- `Expected {expected}, but found {actual}`

## Before

```miri
fn main)
    println("Hello")
```

## After

```miri
fn main()
    println("Hello")
```

## Reference

[Parser](../reference/parser.md)
