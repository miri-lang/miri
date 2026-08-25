# Target-Specific Capabilities and Restrictions

Different compilation targets (CPU vs. GPU) have different capabilities and restrictions. GPU code cannot use arbitrary system calls or host-specific APIs; CPU code cannot use GPU-specific intrinsics. The type checker enforces these restrictions to prevent invalid code generation.

## GPU Restrictions

- GPU code cannot call CPU-only functions
- GPU code cannot use host-specific file I/O or system calls
- Shuffle offsets must not exceed the subgroup size
- Division and modulo operations have range restrictions in GPU kernels
- Barrier control must respect GPU synchronization rules
- Certain types are not directly accelerable to GPU (e.g., reference types)

## CPU Restrictions

- CPU code cannot call GPU-only kernels directly (must use forall or gpu for)
- CPU code cannot use GPU-resident buffers or memory

## Per-Code Detail

Use `miri explain MER_TAR_<code>` for detailed guidance on each target-specific diagnostic code.
