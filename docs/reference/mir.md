# MIR and Lowering

MIR (Mid-level Intermediate Representation) is the compiler's internal representation of the program after type checking. The lowering pass converts the AST into MIR, desugars high-level constructs, and performs control-flow validation. Later passes then write the reference counts onto it before code generation.

## What It Rejects

- Break or continue outside a loop context
- Unsupported expressions or statements (constructs not yet implemented)
- Undefined variables in MIR generation (should not occur if type checker passed)
- Invalid GPU launch arguments or metadata
- Type mismatches that survived type checking (validation failures)
- A generic class instance whose type arguments nest more than 32 type constructors deep, or a class needed at more than 256 value arguments — the marks of polymorphic recursion, an argument that grows on every call, which no finite set of compiled bodies covers
- A generic class instance, built inside a body compiled for one instantiation, whose arguments name none: a value argument that overflows the signed 128-bit range, divides by zero or uses an operator other than `+`, `-`, `*`, `/` and `%` once its parameters are bound
- A generic class instance, generic function call or `gpu fn` launch, anywhere, at a type argument the compiler cannot name — a type nested more than 64 levels deep, or a closure type declaring type parameters of its own
- Two different definitions reached from GPU code that are declared under one kernel-side name, such as a function named `miri_gpu_forall_0` called from the kernel a `forall` compiles to under that same name: kernel-side names admit identifier characters only, while host link names keep every definition apart

## Key Concepts

- **MIR instructions**: Primitive operations (assignment, function calls, arithmetic, memory access)
- **Basic blocks**: Sequences of MIR instructions with control-flow terminators
- **Ownership on the IR**: Reference counts are written on the MIR, never decided by a backend. Whole-program inference decides which parameters a callee keeps (`owned_params`); a local read out of a place the body holds borrows it and takes no count (`local_borrows`); one pass inserts the retains and releases every counted value needs; later passes cancel the pairs with no net effect (`rc_elision`) and the uniqueness checks an earlier check answered (`unshare_elision`). No pass adds a retain, release or copy the source did not name
- **Release placement**: A value whose release nothing can observe is released at its last use; a value whose release may run a `drop` hook, at the point it is consumed or at its owner's scope end in reverse declaration order
- **Device bodies**: A GPU body carries no counts; the host owns every device buffer
- **GPU kernels**: Kernel functions are lowered to WebGPU via WGSL emission

## Per-Code Detail

Use `miri explain MER_MIR_<code>` for detailed guidance on each MIR diagnostic code.
