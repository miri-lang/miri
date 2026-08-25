## Rule

A circular dependency was detected: module A imports module B which (directly or transitively) imports module A back. Circular imports prevent the type checker from producing a stable module load order and must be broken by removing or reorganizing the imports.

## Before

```miri
// file: mod_a.mi
use local.mod_b

fn func_a()
    println("a")

// file: mod_b.mi
use local.mod_a

fn func_b()
    println("b")

// file: main.mi
use local.mod_a

fn main()
    func_a()
```

## After

```miri
// file: mod_a.mi
fn func_a()
    println("a")

// file: mod_b.mi
fn func_b()
    println("b")

// file: main.mi
use local.mod_a

fn main()
    func_a()
```

## Reference

[Imports and Module Loading](../reference/imports.md)
