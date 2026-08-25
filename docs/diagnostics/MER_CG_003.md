## Rule

The Cranelift backend failed to declare a function in the code generation module. This is an internal error that occurs when the backend attempts to define a function signature and the operation fails. It is not triggered by user code.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to declare function 'main': [internal Cranelift error]
```

## After

This is typically an environment or backend configuration issue. Verify that the program's functions have valid signatures and the backend is functioning correctly.

## Reference

[Code Generation](../reference/codegen.md)
