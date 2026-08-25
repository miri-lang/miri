## Rule

The Cranelift backend failed to generate code for a function. This is an internal error that occurs during the function definition phase and is not triggered by user code. It indicates that the backend encountered an issue while translating the function's MIR to executable code.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to define function 'main': [internal Cranelift error]
```

## After

This is typically an environment or backend configuration issue. Ensure that all functions are well-formed and the backend has sufficient resources.

## Reference

[Code Generation](../reference/codegen.md)
