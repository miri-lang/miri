## Rule

The `miri explain` command received a diagnostic code that is not in the registry. Valid diagnostic codes follow the format `MER_<AREA>_<NUM>` where AREA is one of LEX, PAR, NAM, IMP, TYP, OWN, MIR, CG, RT, TAR, or BLD, and NUM is a zero-padded three-digit number. Check the code spelling and use `miri explain` with a valid code.

## Before

```sh
miri explain MER_XYZ_001
```

## After

```sh
miri explain MER_OWN_001
```

## Reference

[Build and Command Line](../reference/build.md)
