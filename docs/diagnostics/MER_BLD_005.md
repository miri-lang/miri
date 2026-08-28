## Rule

`miri view --fn` was given a name that more than one declaration answers to, so there is no single function to show. This happens when two classes in one file declare a method of the same name and the name was given bare. Qualify it with the declaring class to pick one.

## Before

```sh
miri view --fn draw shapes.mi
```

## After

```sh
miri view --fn Circle.draw shapes.mi
```

## Reference

[Build and Command Line](../reference/build.md)
