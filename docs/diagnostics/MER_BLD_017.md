## Rule

`miri patch --insert-fn` was asked to insert a declaration whose name already exists in the file. A name can be declared only once at the same scope. At the top level, a bare function name must not already be declared as a free function. Inside a container, a method name must not already be declared inside that same container. A top-level function `foo` and a method `foo` in a class may coexist, as they occupy different scopes.

## Before

```sh
miri patch file.mi --insert-fn helper --body-file -
# where file.mi already declares a function named helper
```

## After

```sh
miri patch file.mi --insert-fn compute --body-file -
```

## Reference

[Build and Command Line](../reference/build.md)
