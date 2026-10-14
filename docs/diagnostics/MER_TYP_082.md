## Rule

An interpolation's format spec cannot format the value it follows. The spec is the text after `:` in `f"{value:spec}"`, written as in Python: `[[fill]align][0][width][.precision][type]`.

| Part | Meaning |
|---|---|
| `<`, `>`, `^` | left-, right- or centre-align in the field, with an optional fill character before it (`*>8`) |
| `0` | pad a number with zeros after its sign |
| width | the narrowest field, in characters (at most 1000) |
| `.precision` | digits after the point, with type `f` (at most 100) |
| `f` | a number in fixed point; an integer is written as a float |
| `d`, `x`, `X`, `b`, `o` | an integer in decimal, hex, uppercase hex, binary or octal |

Numbers align right by default and everything else left. A width pads any value by the text it would interpolate as.

A precision needs its type: Python reads `{x:.2}` as two significant digits and Rust as two decimals, so Miri asks for `.2f` rather than pick one silently. A radix applies to integers of up to 64 bits, and zero padding to numbers.

## Messages

- `format spec '{spec}' is not one Miri reads`
- `format spec '{spec}' gives a precision without a type`
- `format spec '{spec}' asks for a wider field or more decimals than Miri writes`
- `format spec '{spec}' writes {form}, but the value is '{type}'`

## Help

- `write the spec as [[fill]align][0][width][.precision][type]: {x:.2f} rounds to two decimals, {n:>8} right-aligns in eight columns, {n:08x} writes zero-padded hex`
- `` write `.{digits}f` for {digits} digits after the point ``
- `a width is at most 1000 and a precision at most 100`
- `` convert the value with `as int` first; a radix writes an integer ``
- `{form} applies to {applies}; drop it from the spec`

## Before

```miri
fn main()
    let x = 3.14159
    println(f"{x:.2}")
```

## After

```miri
fn main()
    let x = 3.14159
    println(f"{x:.2f}")
```

## Reference

[Type Checker](../reference/types.md)
