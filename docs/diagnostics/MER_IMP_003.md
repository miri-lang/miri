## Rule

A selective import (using `use module.{name1, name2}` syntax) requests a name that does not exist in that module. Check the module's exported types and functions, and verify the name spelling.

## Before

```miri
// file: module.mi
fn existing_function()
    println("exists")

// file: main.mi
use local.module.{missing_name}

fn main()
    println("hello")
```

## After

```miri
// file: module.mi
fn existing_function()
    println("exists")

// file: main.mi
use local.module.{existing_function}

fn main()
    existing_function()
```

## Reference

[Imports and Module Loading](../reference/imports.md)
