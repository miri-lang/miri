## Rule

A selective import (using `use module.{name1, name2}` syntax) requests a name that does not exist in that module. Check the module's exported types and functions, and verify the name spelling. The selection is checked however the module came to be loaded, including a module the prelude or an implicit import already loaded, and a name close to one the module declares is suggested.

## Messages

- `Name '{name}' not found in module '{module}'`

## Help

- `Did you mean '{suggestion}'?`

## Before

```miri
use system.math.{sqrtt}

fn main()
    println(f"{sqrtt(16.0)}")
```

## After

```miri
use system.math.{sqrt}

fn main()
    println(f"{sqrt(16.0)}")
```

## Reference

[Imports and Module Loading](../reference/imports.md)
