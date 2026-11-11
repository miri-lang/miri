## Rule

A resource was consumed and then used again. A resource (a class or enum with a `drop` hook) has one owner: assigning it moves it, and passing it to a function that keeps it (stores it, returns it, captures it) gives that function ownership. The name the resource came from cannot be read afterwards. A call that only reads its argument lends it, and every other type is lent by every call, so only a kept resource is ever consumed.

The report explains why the resource was consumed: each step from the call to the place that keeps the resource is attached as a note, pointing at the line that takes it.

## Messages

- `'{var}' was consumed by '{sink}' and cannot be used again\n  fix: use '{var}' before the call that keeps it, or pass it to a function that only reads it`

## Before

```miri
class Resource
    var id int
    fn drop(self)
        return

fn keep(r Resource) Resource
    return r

fn main()
    let r = Resource(id: 1)
    let kept = keep(r)
    println(f"{r.id} {kept.id}")
    kept.drop()
```

## After

```miri
class Resource
    var id int
    fn drop(self)
        return

fn keep(r Resource) Resource
    return r

fn main()
    let r = Resource(id: 1)
    println(f"{r.id}")
    let kept = keep(r)
    println(f"{kept.id}")
    kept.drop()
```

## Reference

[Ownership and Resource Management](../reference/ownership.md)
