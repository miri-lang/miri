# Code Generation

Code generation translates MIR into backend instructions. The default backend is Cranelift, which compiles to machine code for the host platform. GPU code is emitted as WebGPU/WGSL. The codegen pass validates that the MIR is translatable and reports backend-specific errors.

## What It Rejects

- Unsupported backends for the target platform
- Invalid or incompatible MIR patterns for the backend
- Object file emission failures
- Target ISA or module creation errors
- Internal codegen invariant violations

## Key Concepts

- **Cranelift backend**: Fast, portable compilation to x86-64, ARM, and RISC-V
- **WGSL backend**: Emit GPU kernels as WebGPU shaders
- **Target ISA**: Instruction set configuration for the host platform
- **Object files**: Intermediate compilation artifacts linked with runtime libraries

## Per-Code Detail

Use `miri explain MER_CG_<code>` for detailed guidance on each code-generation diagnostic code.
