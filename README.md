# Miri — a GPU-first programming language

<p align="center">
  <img src="banner.png"/>
</p>

**A modern, GPU-first, statically-typed, compiled programming language designed for balancing high performance and safety in the age of Generative AI.**

Miri is built for **agentic engineering** — a world in which the majority of production code is generated, repaired, and shipped by autonomous agents. Humans declare intent. The compiler enforces the invariants agents are most likely to violate, and the toolchain emits structured artifacts agents can consume directly.

## GPU, from the language up

Every animation below is a real Miri program — plain `forall` loops over `gpu`-resident buffers, compiled to WGSL and run on the GPU. No host/device boilerplate.

<p align="center">
  <a href="https://miri-lang.org/gpu-demos/"><img src="media/demos/blackhole.gif" width="49%" alt="Black hole — photon geodesics & gravitational lensing"/></a>
  <a href="https://miri-lang.org/gpu-demos/"><img src="media/demos/mandelbrot.gif" width="49%" alt="Mandelbrot set"/></a>
  <a href="https://miri-lang.org/gpu-demos/"><img src="media/demos/raymarch.gif" width="49%" alt="Ray-marched signed-distance scene"/></a>
  <a href="https://miri-lang.org/gpu-demos/"><img src="media/demos/life.gif" width="49%" alt="Conway's Game of Life"/></a>
</p>

<p align="center"><b><a href="https://miri-lang.org/gpu-demos/">▶ Run all eight demos live in your browser →</a></b></p>

```miri
gpu let a = [1.0, 2.0, 3.0, 4.0]
gpu let b = [5.0, 6.0, 7.0, 8.0]
gpu var dst = [0.0, 0.0, 0.0, 0.0]

forall i in 0..a.length()
    dst[i] = a[i] + b[i]

let host = dst // the only boundary crossing: assignment = readback
println(f"{host[0]} {host[1]} {host[2]} {host[3]}") // 6.0 8.0 10.0 12.0
```

## Current State (v0.5.0-beta.3)

This release is the **GPU Preview**. Miri compiles data-parallel code to WebGPU (WGSL) and runs it on real hardware — driven entirely from the language, with no separate shading language and no manual buffer marshalling.

**New in v0.5.0-beta.3:**
- **Residency surface** — `gpu let` / `gpu var` mark buffers that live on the device; the `Accelerable` trait gates which types are eligible. Cross-residency assignment is the *only* boundary crossing (`let host = dst` reads back, host→device re-uploads), with copy semantics and compile-time rejection of accidental element cross-reads.
- **`forall` / `gpu forall`** — data-parallel loops in 1-, 2-, and 3-D, with literal or runtime bounds. Bare `forall` runs on the CPU unless GPU residency is inferred. Explicit `gpu forall` runs on the GPU.
- **`gpu fn` kernels** — device functions with `kernel.*` builtins (`global_idx`, `thread_idx`, `block_idx`, `barrier()`), `shared` workgroup memory, `Atomic<i32>` / `Atomic<u32>`, subgroup ops, and on-device `.reduce`.
- **GPU vectors** — `Vec2` / `Vec3` / `Vec4<f32>` with `dot`, `length`, `normalize`, `cross`, `reflect`, `mix`, scalar broadcast, and std430-correct buffer storage.
- **Backend + tooling** — a WGSL codegen backend, a `wgpu` host runtime, `miri build --target web-gpu` (emits a runnable WebGPU bundle + native host binary), and a browser Tint validation gate in CI.
- **Eight interactive web demos** — Mandelbrot, Game of Life, particle flow, fluid, ray marching, an on-GPU neural net, and gravitationally-lensed black-hole / wormhole shaders, each a compile-verified Miri program.

Building on the CPU language from prior betas: full Perceus memory safety, `Result<T, E>` with enforced `must_use`, a backend-agnostic `system.math`, the four-trait collection pipeline, and `system.testing`.

**Why WGSL first — and what's next.** WGSL/WebGPU is the *first* GPU backend, not the only planned one. It shipped first because it's portable and runs everywhere — including the browser, so the demos above need zero install — and it let us prove the whole language→GPU path (residency, `forall`, kernels, codegen, host runtime) end-to-end against a single target before multiplying backends. The GPU representation in MIR is being made backend-neutral so additional backends — a native SPIR-V / Vulkan path and others — can plug in behind the exact same `forall` / `gpu fn` surface, with no changes to your source.

## Language at a glance

```miri
use system.io

struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    println(f"{p.x}, {p.y}")
```

Value semantics are enforced — assignment is a logical copy, mutation never aliases, and memory is managed with zero annotations (only `out` for in-place mutation):

```miri
use system.collections.list

fn main()
    let a = List([1, 2, 3])
    var b = a            // copy-on-write share
    b.push(4)            // CoW fires: b becomes independent
    println(f"{a.length()} {b.length()}")   // 3 4
```

Resource types (those defining `fn drop(self)`) are tracked strictly — using one after it is consumed is a compile error, with a multi-hop diagnostic explaining *why*:

```miri
fn main()
    let f = File(handle: 1)
    archive(f)
    archive(f)           // compile error: 'f' was consumed by 'archive'
```

Miri also ships classes with inheritance and virtual dispatch, traits with default methods, closures, generics with monomorphization, pattern matching, `Option`/`Result`, tuples, enums with data, and a multi-file module system with cross-module visibility.

**→ Full language tour, GPU guide, and API docs at [miri-lang.org](https://miri-lang.org).**

## Architecture

Miri follows a standard compiler pipeline:

```text
Source(s) → Lexer → Parser → AST → Type Checker → MIR → Codegen → Executable
                                                    │
                                                    ├─ Cranelift  → native binary
                                                    └─ WGSL       → WebGPU
```

The `Pipeline` struct in `src/pipeline.rs` orchestrates discovery, frontend (lex/parse), analysis (type checking + cross-module visibility), MIR lowering with optimization passes (including the Perceus reference-counting transform), and backend codegen. On the CPU side, Cranelift is the default backend and an LLVM backend is planned for optimized production builds. On the GPU side, WGSL is the current backend; the MIR GPU representation is being made backend-neutral so native paths (SPIR-V / Vulkan and beyond) can be added behind the same surface.

## Repository Layout

```bash
src/
├── ast/          # Syntax tree definitions
├── cli/          # Command-line interface
├── codegen/      # Backend implementations (Cranelift, WGSL)
├── error/        # Error types and formatting
├── lexer/        # Source tokenization
├── mir/          # IR definitions, lowering, and optimization passes
├── parser/       # Parsing logic
├── runtime/      # Runtime intrinsics (core + GPU) — separate staticlib crates
├── stdlib/       # Standard Library (system.*), written in Miri
├── type_checker/ # Type inference, validation, residency analysis
└── pipeline.rs   # Main compiler driver
```

## Building & Testing

Miri is written in Rust. Build with a stable Rust toolchain:

```bash
make build        # Build compiler + all runtime crates (debug)
make release      # Build compiler + all runtime crates (release) → target/release/miri
make test         # Run the full suite (compiler, stdlib, runtimes)
make lint         # Check formatting + clippy
make format       # Auto-format all code
```

## Contributing

We welcome contributions! Please read our [Contributing Guide](CONTRIBUTING.md) for details on code style, testing requirements, and the submission process.

### Contributors

- Viacheslav Shynkarenko aka Slavik Shynkarenko (maintainer)

## License

[Apache-2.0](LICENSE)
