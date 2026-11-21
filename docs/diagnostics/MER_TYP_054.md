## Rule

A function's signature, or a call against it, is invalid. This covers several family members: incorrect parameter types, incompatible return types, invalid generic constraints, a missing return, and an argument to an `out` parameter that names no place the call can take. The message carries the specific case.

An argument to an `out` parameter is a place: the call takes it, the callee writes it, and the new value is there when the call returns. A place starts at a `var`, at an `out` parameter of the function making the call, or at a method's own `self`, and may then go through fields and elements: `step(grid)`, `step(grid[i])`, `replace(self.slot, None)`, `insert(cur.left, v)`. A value no variable holds, such as a call's result or a literal, is no place; bind it to a `var` first and pass that variable or a place in it.

A map entry is never a place. A map reaches an entry only through its key, so a call cannot write the entry in place. The same holds for a runtime function, which writes the variable it is handed and reaches no place inside one. Copy in, copy out: copy the value into a `var`, pass the `var`, and store it back (`var value = cells[k]`, `step(value)`, `cells[k] = value`). That is the slower spelling, because it copies the value twice, but it works wherever a place path does not. A slot of a tuple is no place either: a tuple's elements are fixed when it is built. Copy the slot into a `var`, pass that, and build the tuple again with it.

Device code takes three kinds of place: a whole local (`bump(t)`), an element of a buffer (`bump(dst[i])`) and a component of a vector (`bump(v.x)`, `bump(pts[i].x)`). The call copies the place in and stores it back. Any other path, such as an element of an element (`grid[0][1]`), is refused; copy it into a `var`, pass that, and store it back. An element of a buffer handed `out` is a write like any other: its index must be one only its own thread reaches. An element of an atomic buffer is never handed `out`: the call would read it and store it back in two steps, and another thread may write it in between; change it with an atomic builtin, which reads and writes it in one step (`atomic_add(iv, i, 1)`). A parameter whose type is a launch context (`Dim3`, `Kernel`, `GpuContext`, `FrameInput`) is never `out` in a function device code calls: the device provides the context and code only reads it.

On the host, a place inside a `gpu var` is refused: the buffer lives in device memory, and a host call cannot take a place in it. Copy the buffer to the host, pass the place in the copy, and copy it back (`var host = dst`, `bump(host[1])`, `dst = host`). That is the only error reported for that argument.

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
- `'out' parameter '{param}' is handed '{place}' in device code, which takes only a whole local, an element of a buffer ('dst[i]') or a component of a vector ('v.x', 'pts[i].x')`
- `'out' parameter '{param}' is handed '{place}', a place in the gpu-resident '{name}', which a call on the host cannot take`
- `'out' parameter '{param}' is handed '{place}', an element of the atomic buffer '{buffer}', which no call takes: the call would read the element and store it back in two steps, and another thread may write it in between`
- `'out' parameter '{param}' of '{func}' has type '{type}', a launch context device code only reads: no call writes one`
- `'out' parameter '{param}' is handed '{place}', a slot of the tuple '{tuple}', which is not a place: a tuple's elements are fixed when it is built`

## Help

- `bind the result to a 'var' first ('var value = {call}'), then pass '{place}'`
- `hold the value in a 'var' ('var value = …'), pass 'value', and read the new value from it after the call`
- `copy the entry into a 'var', pass that and store it back under its key: 'var value = {entry}', pass '{place}', then '{entry} = value'`
- `copy it into a 'var', pass that and store it back: 'var value = {place}', pass 'value', then '{place} = value'`
- `copy '{name}' to the host ('var host = {name}'), pass '{place}', then copy it back ('{name} = host')`
- `change the element with an atomic builtin, which reads and writes it in one step: 'atomic_add({buffer}, {index}, 1)', or another 'atomic_' operation`
- `drop 'out' from '{param}' and return what the function computes, or write it through an 'out' parameter of a scalar type`
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
