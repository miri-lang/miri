## Rule

The file named on the command line could not be read. The path may not exist, may name a directory, or may not be readable by the current user. This reports the failure to open the input; it says nothing about the contents of any Miri program.

## Before

```sh
miri view --outline src/mian.mi
```

## After

```sh
miri view --outline src/main.mi
```

## Reference

[Build and Command Line](../reference/build.md)
