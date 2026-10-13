## Rule

A selective import (using `use module.{name1, name2}` syntax) requests a name that does not exist in that module. Check the module's exported types and functions, and verify the name spelling.

## Messages

- `Name '{name}' not found in module '{module}'`

## Before

```miri
use system.math.{square_root}

fn main()
    println(f"{square_root(16.0)}")
```

## After

```miri
use system.math.{sqrt}

fn main()
    println(f"{sqrt(16.0)}")
```

## Reference

[Imports and Module Loading](../reference/imports.md)
