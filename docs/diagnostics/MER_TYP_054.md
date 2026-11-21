## Rule

A function's signature, or a call against it, is invalid. This covers several family members: incorrect parameter types, incompatible return types, invalid generic constraints, a missing return, and an argument to an `out` parameter that names no place the call can take. The message carries the specific case.

An argument to an `out` parameter is a place: the call takes it, the callee writes it, and the new value is there when the call returns. A place starts at a `var`, at an `out` parameter of the function making the call, or at a method's own `self`, and may then go through fields and elements: `step(grid)`, `step(grid[i])`, `replace(self.slot, None)`, `insert(cur.left, v)`. A value no variable holds, such as a call's result or a literal, is no place; bind it to a `var` first and pass that variable or a place in it.

A map entry is never a place. A map reaches an entry only through its key, so a call cannot write the entry in place. The same holds for a runtime function, which writes the variable it is handed and reaches no place inside one, and for device code, where a function takes no `out` parameter at all. Copy in, copy out: copy the value into a `var`, pass the `var`, and store it back (`var value = cells[k]`, `step(value)`, `cells[k] = value`). That is the slower spelling, because it copies the value twice, but it works wherever a place path does not. In device code, have the function take the value as a plain parameter and return the new one, then store the result. A slot of a tuple is no place either: a tuple's elements are fixed when it is built. Copy the slot into a `var`, pass that, and build the tuple again with it.

## Messages

- `Missing return statement`
- `conflict: parameter '{param}' cannot be explicitly marked 'host' in a gpu function`
- `Regex literals cannot be used inside a GPU function; use Regex.compile() at host level and pass it as a parameter`
- `parameter '{param}' is explicitly marked 'gpu' but received host-resident argument`
- `'out' parameter '{param}' is handed '{place}', a place in the result of a call, which no variable holds: a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', the result of a call, which no variable holds: a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed a value, which no variable holds: a call writes only a place rooted at a 'var', an 'out' parameter or 'self'`
- `'out' parameter '{param}' is handed '{place}', an entry of the map '{map}', which is not a place: a map reaches an entry only through its key, so a call cannot write the entry in place`
- `'out' parameter '{param}' is handed '{place}', a place in an entry of the map '{map}', which is not a place: a map reaches an entry only through its key, so a call cannot write the entry in place`
- `'out' parameter '{param}' of a runtime function takes a whole variable, not '{place}': the runtime writes the variable it is handed and reaches no place inside one`
- `'out' parameter '{param}' is handed '{place}' in device code, where a call takes no place`
- `'out' parameter '{param}' is handed '{place}', a place in the gpu-resident '{name}', which a call on the host cannot take`
- `'out' parameter '{param}' is handed '{place}', a slot of the tuple '{tuple}', which is not a place: a tuple's elements are fixed when it is built`

## Help

- `bind the result to a 'var' first ('var value = {call}'), then pass '{place}'`
- `hold the value in a 'var' ('var value = …'), pass 'value', and read the new value from it after the call`
- `copy the entry into a 'var', pass that and store it back under its key: 'var value = {entry}', pass '{place}', then '{entry} = value'`
- `copy it into a 'var', pass that and store it back: 'var value = {place}', pass 'value', then '{place} = value'`
- `take the value as a plain parameter and return the new one, then store it: '{place} = …({place})'`
- `copy '{name}' to the host ('var host = {name}'), pass '{place}', then copy it back ('{name} = host')`
- `copy it into a 'var' and pass that: 'var value = {place}', pass 'value', then build '{tuple}' again with 'value' in place of '{place}'`

## Before

```miri
use system.collections.list

class Cell
    var n int

fn step(c out Cell)
    c = Cell(n: c.n + 10)

fn make() List<Cell>
    return List([Cell(n: 1)])

fn main()
    step(make()[0])
```

## After

```miri
use system.io
use system.collections.list

class Cell
    var n int

fn step(c out Cell)
    c = Cell(n: c.n + 10)

fn make() List<Cell>
    return List([Cell(n: 1)])

fn main()
    var value = make()
    step(value[0])
    println(f"{value[0].n}")
```

## Reference

[Type Checker](../reference/types.md)
