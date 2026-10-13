## Rule

`miri view --fn` was asked for a function the file does not declare, or `miri patch --insert-fn Class.method` was asked to insert a method into a class the file does not declare. The name must match a function declared in the file being viewed: a free function by its bare name, or a method by its `Class.method` form. A method cannot be reached by its bare name, because the same method name may be declared by several classes in one file. For insertions, the container (class, struct, enum, or trait) must exist in the file being patched.

## Messages

- `` no function named `{name}` in this file ``

## Help

- `` a method is reached as `Class.method`; run `miri view --outline` to list what the file declares ``
- `` `{name}` is a {kind}, not a function; run `miri view --type {name}` to list what it declares ``

## Before

`hello.mi`:

```miri
fn length_of(word String) int
    return word.length()
```

```sh
miri view --fn lenght hello.mi
```

## After

```sh
miri view --fn length_of hello.mi
```

## Reference

[Build and Command Line](../reference/build.md)
