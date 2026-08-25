# MIR and Lowering

MIR (Mid-level Intermediate Representation) is the compiler's internal representation of the program after type checking. The lowering pass converts the AST into MIR, desugars high-level constructs, and performs control-flow validation. The Perceus pass then optimizes reference counting before code generation.

## What It Rejects

- Break or continue outside a loop context
- Unsupported expressions or statements (constructs not yet implemented)
- Undefined variables in MIR generation (should not occur if type checker passed)
- Invalid GPU launch arguments or metadata
- Type mismatches that survived type checking (validation failures)

## Key Concepts

- **MIR instructions**: Primitive operations (assignment, function calls, arithmetic, memory access)
- **Basic blocks**: Sequences of MIR instructions with control-flow terminators
- **Perceus optimization**: Reference-count insertion and elision based on escape analysis
- **GPU kernels**: Kernel functions are lowered to WebGPU via WGSL emission

## Per-Code Detail

Use `miri explain MER_MIR_<code>` for detailed guidance on each MIR diagnostic code.
