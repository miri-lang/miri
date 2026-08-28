## Rule

`miri view --fn` was asked for a function the file does not declare. The name must match a function declared in the file being viewed: a free function by its bare name, or a method by its `Class.method` form. A method cannot be reached by its bare name, because the same method name may be declared by several classes in one file.

## Before

```sh
miri view --fn lenght examples/hello.mi
```

## After

```sh
miri view --fn length examples/hello.mi
```

## Reference

[Build and Command Line](../reference/build.md)
