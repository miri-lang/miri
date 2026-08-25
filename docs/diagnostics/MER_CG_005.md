## Rule

The Cranelift backend failed to translate MIR to its internal representation. This is an internal error that occurs during the MIR-to-backend translation phase and is not triggered by user code. It indicates that the backend encountered an instruction or construct it cannot process.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to translate function 'main': [internal Cranelift error]
```

## After

This is typically an environment or backend configuration issue. Ensure that the program's MIR is well-formed and the backend supports all required instructions.

## Reference

[Code Generation](../reference/codegen.md)
