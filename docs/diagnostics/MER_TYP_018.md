## Rule

Some attributes require a companion attribute to be present on the same declaration. For example, `@ignore` and `@xfail` are only valid on functions already marked with `@test`. This error is raised when a dependent attribute is used without its required companion.

## Before

```miri
@ignore("flaky")
fn not_a_test()
    println("not a test")
```

## After

```miri
@test
@ignore("flaky")
fn ignored_test()
    println("test")
```

## Reference

[Type Checker](../reference/types.md)
