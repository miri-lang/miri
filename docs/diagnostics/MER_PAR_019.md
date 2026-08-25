## Rule

An attribute is being applied to a declaration type that does not support attributes. Attributes (marked with `@`) can only precede enum, function, or class declarations. Applying an attribute to a variable, statement, or other construct causes this error.

## Before

```miri
@deprecated
let old_value = 42

@must_use
var mutable_flag = true
```

## After

```miri
let old_value = 42

@must_use
fn important() bool
    return true
```

## Reference

[Parser](../reference/parser.md)
