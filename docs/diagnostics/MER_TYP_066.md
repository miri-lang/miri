## Rule

An `out` parameter was used incorrectly. A call takes the place it hands to an `out` parameter, and takes each variable once: two `out` arguments may not start at the same variable, whether both name it whole or both name places inside it. `swap(v[i], v[j])` is refused because nothing proves `i` and `j` differ, and when they do not, both writes land in one element. The rule is per variable, not per place: `fill(h.tags, h.small)` is refused too, though its two fields are different places. Copy one of the two places into a `var`, pass that, and store it back after the call. An `out` argument beside an ordinary one that reads the same variable is accepted: `f(v[i], v[j])` with only the first parameter `out` hands the second the value `v[j]` held before the call.

A call may not lend a value containing a place it writes. When an `out` argument is a field or an element, no other argument, and not the value a method is called on, may name the variable it starts at or a path toward it: `put(h.tags, h)`, `h.swap_from(h.conn)`, `put(v[0].conn, v)` and `bump(v[i], v)` are refused, and so is `put(v[i].conn, v[j])`, since nothing proves `i` and `j` differ. The callee would read the place through that value while it writes the place. Copy the place into a `var`, pass that, and store it back after the call. A sibling that is not on the way to the place, `rename(h.tags, h.name)`, is accepted. The place itself lent beside its `out` argument is refused the same way: `f(x, x)`, `f(p.n, p.n)` and `c.absorb(c)`, since the callee would read a copy of a value it is writing. Pass the place once, and lend what the call reads from it as a value of its own, made before the call: `let before = x`, then `f(x, before)`. Two elements of one collection are siblings rather than one place, so `f(v[i], v[j])` is still accepted.

An override must also declare `out` on the same parameters as the method it overrides.

## Messages

- `same variable passed twice as 'out': '{name}' appears more than once`
- `same variable passed twice as 'out': '{first}' and '{second}' are both places in '{name}', and a call takes a variable once`
- `'out' parameter '{param}' is handed '{place}', and the call also lends '{value}', a value holding that place: a call may not lend a value containing a place it writes`
- `'out' parameter '{param}' is handed '{place}', and the call is made on '{value}', a value holding that place: a call may not lend a value containing a place it writes`
- `'out' parameter '{param}' is handed '{place}', and the call also lends '{value}', the same place: a call may not lend a value containing a place it writes`
- `'out' parameter '{param}' is handed '{place}', and the call is made on '{value}', the same place: a call may not lend a value containing a place it writes`
- `Method '{method}' has incompatible 'out' modifier for parameter '{param}' (position {position}): parent declares {parent}, child declares {child}`

## Help

- `pass a different 'var' to each 'out' parameter`
- `a call takes each variable once, whichever places in it the arguments name; copy '{place}' into a 'var' ('var value = {place}'), pass 'value', then store it back ('{place} = value')`
- `copy it into a 'var', pass that and store it back: 'var value = {place}', pass 'value', then '{place} = value'`
- `pass '{place}' once, and read what the other argument needs from it before the call: for a value, 'let before = {place}', then pass 'before' where '{place}' is lent`

## Before

```miri
use system.collections.list

fn swap(a out int, b out int)
    let t = a
    a = b
    b = t

fn main()
    var v = List([1, 2, 3])
    let i = 0
    let j = 2
    swap(v[i], v[j])
```

## After

```miri
use system.io
use system.collections.list

fn swap(a out int, b out int)
    let t = a
    a = b
    b = t

fn main()
    var v = List([1, 2, 3])
    let i = 0
    let j = 2
    var value = v[j]
    swap(v[i], value)
    v[j] = value
    println(f"{v[0]} {v[1]} {v[2]}")
```

## Reference

[Type Checker](../reference/types.md)
