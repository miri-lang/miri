## Rule

An assignment was attempted to a variable or field declared as immutable. Variables declared with `let` cannot be reassigned; use `var` to create a mutable binding if reassignment is needed.

Writing a field or an element through a `let` binding (`p.x = 5`, `xs[0] = 9`) is the same mistake. `miri fix` rewrites the declaration's `let` as `var` through the `let-to-var` repair whether the binding itself is reassigned or something is written through it. A parameter or a constant has no `let` to rewrite and gets no repair.

A field its class declares with `let` is fixed once the instance is built: only the class's own `init` sets it, on `self`. Assigning it anywhere else, through any binding and in a subclass too, is refused; declare the field with `var` if it must change.

Handing a place to an `out` parameter is a write too, so a field or an element of a `let` (`step(grid[0])`) is refused the same way and gets the same repair. A place reached from a parameter the caller lends is repaired by declaring that parameter `out`, which writes the caller's value, or by copying it into a `var`, writing the copy and returning it. A `for` loop's binding is not a place a call can take; iterate by index and pass the element through the collection (`for i in 0..rows.length()`, then `step(rows[i][0])`).

## Messages

- `Cannot assign to immutable variable '{var}'`
- `Cannot assign to field of immutable variable`
- `Cannot assign to element of immutable variable`
- `Cannot assign to field '{field}': its class declares it with 'let'`
- `Invalid assignment target`
- `'out' parameter '{param}' is handed '{place}', but '{var}' is immutable: it is declared with 'let', and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', but '{var}' is immutable: it is a parameter its caller lends, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', but '{var}' is immutable: it is a constant, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', but '{var}' is immutable: it is the binding of a 'for' loop, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', a path through the field '{field}', which its class declares with 'let'`

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
