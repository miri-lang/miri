# Miri Development Plan

## **Objective**
Develop Miri incrementally with a focus on delivering an early **Minimum Lovable Product (MLP)** that provides immediate value while setting the foundation for future expansion. The approach will be **agile and iterative**, ensuring continuous feedback and course correction.

## **Development Phases and Milestones**

### **Phase 1: Research & Planning (Month 1-2)**
**Goal:** Establish a solid technical foundation and ensure feasibility before starting development.
- [x] Define **core syntax**, type system, and programming model.
- [x] Research Rust-based compiler development (LLVM integration, parsing, code generation).
- [x] Establish **development environment** (repository, CI/CD pipeline, build tools).
- [x] Identify and document key differentiators vs. Rust, C++, and Python.

**Deliverable:** A detailed specification document, initial roadmap, and repository setup.

Done ✅

---

### **Phase 2: MLP Compiler Prototype (Month 3-5)**
**Goal:** Build a functional, minimal version of the language.
- [x] Implement **lexical analysis and parsing**.
  - [x] use
  - [x] type expression
  - [x] generic functions
  - [x] generic classes
  - [x] break/continue
  - [x] enums
  - [x] structs
  - [x] using enums, structs and functions from other modules e.g. Http::Status.Ok, Http::get
  - [x] async/await
  - [x] gpu
  - [x] private/protected
  - [x] lambda, function as variables and parameters
  - [x] extend/implement/include
  - [x] list
  - [x] tuple
  - [x] map
  - [x] set
  - [x] regex
  - [x] match / pattern matching
  - [x] string interpolation
  - [x] refactor lexer & parser tests (split them in modules)
  - [x] identify gaps in tests
  - [x] optimize parser
- [x] Create an **AST (Abstract Syntax Tree)** representation.
- [ ] Introduce a simple **REPL** (interactive shell for experimentation, for now just returns the AST).
- [ ] Add simple integration tests (running real programs, without failing)
- [] Implement a basic **type checker** (static type system enforcement).
- [] Compile basic expressions to LLVM IR for execution.
- [] Create unit tests for core language features.

**Deliverable:** A working prototype capable of interpreting and compiling simple programs.

---

### **Phase 3: Basic Compiler with Concurrency & Safety (Month 6-8)**
**Goal:** Build a usable version of the language with core safety and concurrency features.
- ✅ Introduce **ownership & borrowing model** for memory safety.
- ✅ Implement **pattern matching & algebraic data types**.
- ✅ Add **immutable data structures** and function-level scoped variables.
- ✅ Introduce **basic concurrency model (async/await, lightweight threads)**.
- ✅ Compile real-world programs to machine code.
- ✅ Publish first benchmarks vs. Python and Rust.

**Deliverable:** MiriLang 0.1 - A minimal but usable language.

---

### **Phase 4: Developer Tooling & First Real-World Use Case (Month 9-12)**
**Goal:** Enable developers to use MiriLang effectively and apply it to a real-world problem.
- ✅ Develop a **package manager & module system**.
- ✅ Build a **standard library (I/O, math, concurrency, collections)**.
- ✅ Introduce **compile-time testing** as part of the language.
- ✅ Release a **basic VSCode plugin** (syntax highlighting, basic linting).
- ✅ Create an **example project using MiriLang (AI/ML focus)**.
- ✅ Engage developers to try MiriLang and gather feedback.

**Deliverable:** MiriLang 0.2 - A functional language with basic developer tooling.

---

### **Phase 5: Performance Optimization & Hardware Utilization (Month 13-18)**
**Goal:** Ensure MiriLang is competitive with Rust/C++ in terms of performance.
- ✅ Improve compiler **optimization passes (LLVM IR tuning)**.
- ✅ Implement **automatic parallelization & SIMD vectorization**.
- ✅ Introduce **GPU acceleration support** for AI/ML workloads.
- ✅ Optimize **incremental compilation times**.
- ✅ Gather real-world benchmarks and improve performance accordingly.

**Deliverable:** MiriLang 0.5 - A high-performance language with first-class parallelism.

---

### **Phase 6: Expanding Ecosystem & Production Readiness (Month 19-24)**
**Goal:** Build confidence in MiriLang as a reliable and efficient alternative to Rust/Python.
- ✅ Extend standard library with **networking, file system, threading APIs**.
- ✅ Introduce **FFI (Foreign Function Interface)** for Rust, C++ integration.
- ✅ Improve error handling and debugging tools.
- ✅ Develop **a community-driven documentation site & tutorials**.
- ✅ Implement **language stability guarantees**.
- ✅ Create **a real-world AI/ML library** in MiriLang.

**Deliverable:** MiriLang 1.0 - A production-ready language for high-performance applications.

---

## **Post-Launch: Long-Term Roadmap (Year 3+)**
**Scaling adoption and industry usage:**
- 🚀 Improve GPU/TPU support for AI/ML.
- 🚀 Expand tooling (advanced IDE support, profiling tools, package ecosystem).
- 🚀 Support WebAssembly & embedded targets.
- 🚀 Establish enterprise adoption (scientific computing, aerospace, defense).

---

## **Summary of Key Milestones**
| Phase  | Duration | Key Deliverables |
|--------|---------|-----------------|
| Research & Planning | 1-2 months | Specification, repo setup |
| MLP Compiler Prototype | 3-5 months | Lexing, parsing, AST, type checker |
| Basic Compiler with Concurrency & Safety | 6-8 months | Ownership model, immutability, async/await |
| Developer Tooling & First Use Case | 9-12 months | Standard library, VSCode support, real-world project |
| Performance Optimization | 13-18 months | Optimized compilation, SIMD, GPU support |
| Production Readiness | 19-24 months | Full standard library, FFI, industry benchmarks |


