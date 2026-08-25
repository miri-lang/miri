## Rule

The Cranelift backend failed to emit the final object file. This is an internal error that occurs after code generation is complete, during the object file serialization phase. It is not triggered by user code and indicates a file I/O or backend serialization issue.

## Before

```
miri build --cpu-backend cranelift program.mi
# Error: Failed to emit object file: [internal Cranelift error]
```

## After

Verify that the output directory is writable and has sufficient disk space. This is typically a file system or environment configuration issue.

## Reference

[Code Generation](../reference/codegen.md)
