## Rule

The Cranelift backend failed to create a target instruction set architecture (ISA). This is an internal error that occurs during backend initialization and is not triggered by user code. The error indicates a misconfiguration of the backend or an issue with the target platform specification.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to create target ISA: [internal Cranelift error]
```

## After

Verify that the target platform is supported and the backend is correctly configured. This is typically a deployment or environment issue, not a source code problem.

## Reference

[Code Generation](../reference/codegen.md)
