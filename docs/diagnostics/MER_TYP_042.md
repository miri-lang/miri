## Rule

An assignment was attempted to a variable or field declared as immutable. Variables declared with `let` cannot be reassigned; use `var` to create a mutable binding if reassignment is needed.

Writing a field or an element through a `let` binding (`p.x = 5`, `xs[0] = 9`) is the same mistake. `miri fix` rewrites the declaration's `let` as `var` through the `let-to-var` repair whether the binding itself is reassigned or something is written through it. A parameter or a constant has no `let` to rewrite and gets no repair.

## Messages

- `Cannot assign to immutable variable '{var}'`
- `Cannot assign to field of immutable variable`
- `Cannot assign to element of immutable variable`
- `Invalid assignment target`
- `expected mutable variable for 'out' parameter '{param}': '{var}' is immutable (declare with 'var')`

## Before

```miri
fn main() i32:
  let x = 5
  x = 10
  0
```

## After

```miri
fn main() i32:
  var x = 5
  x = 10
  0
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
