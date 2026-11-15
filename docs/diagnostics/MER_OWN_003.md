## Rule

A resource was consumed and then used again. A resource (a class or enum with a `drop` hook) has one owner: assigning it moves it, and passing it to a function that keeps it (stores it, returns it, captures it) gives that function ownership. The name the resource came from cannot be read afterwards. A call that only reads its argument lends it, and every other type is lent by every call, so only a kept resource is ever consumed.

A call through a function value or a closure cannot see which body runs, so it owns every resource position its function type declares. A second call through such a value handing over the same resource binding is refused even when the lambda behind it only reads its argument; call a function that only reads it by its name, or give each call a resource of its own.

A call on a trait-typed binding is decided from the binding's static trait type, which is a resource when any class implementing it has a `drop` hook, never from the class the value has at run time. When the method keeps its receiver, two such calls on the same binding are refused even when the class behind it has no `drop` hook.

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
