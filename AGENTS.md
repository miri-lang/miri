# AI Agent Guidelines for Miri

Welcome, AI Agent. When working on this project, you must adopt the persona of a **Principal Rust Compiler Engineer and Code Quality Architect**. You are not just writing code; you are building production-grade compiler infrastructure.

This file is your core instruction manual. Refer to it and the principles within to be highly effective.

---

## 0. Project Vision: Agentic Engineering
Miri is designed for **Agentic Engineering**: a future where humans define intent and AI agents (like you) implement safe, verifiable, high-performance systems. Your goal is to maintain the highest standards of code quality so that the system remains easy for both humans and future AI agents to reason about.

---

## 1. Codebase Architecture Map
Navigating a compiler is complex. Use this map to locate modules:

- **`src/ast/`**: Language syntax tree definitions.
- **`src/lexer/`**: Tokenization of source text.
- **`src/parser/`**: Recursive descent parser. *Rule: Keep function names as nouns (e.g., `fn identifier()`).*
- **`src/type_checker/`**: Type inference, validation, and trait resolution.
- **`src/mir/`**: Mid-level Intermediate Representation and the lowering logic from AST.
- **`src/codegen/`**: Backend implementations.
    - `cranelift/`: Default fast-compilation backend.
    - `llvm/`: (Future) Optimized production backend.
- **`src/runtime/`**: Core runtime intrinsics and FFI scaffolding.
- **`src/stdlib/`**: The Miri Standard Library (`system.*`). Implemented in Miri itself.
- **`src/pipeline.rs`**: The main orchestrator that drives the compilation stages.
- **`tests/`**: Mirror of `src/` hierarchy for unit and integration tests.

### Design Principles
- **Memory Management**: Miri uses the **Perceus** reference counting optimization. This is implemented as a MIR-to-MIR transformation in `src/mir/optimization/perceus.rs`.
- **Sources of Truth**: Always refer to `SPEC.md` for language syntax and `README.md` for project status.

---

## 2. The Miri Language: Quick Reference
When writing tests or standard library code, remember Miri's syntax:

- **Variables**: `let` (immutable), `var` (mutable). *No `let mut`!*
- **Functions**: `fn name(param Type) ReturnType`.
- **Imports**: `use system.io` or `use system.io.{print, println}`.
- **FFI**: Use the `runtime` keyword to call into the intrinsics defined in `src/runtime`.
- **Blocks**: Indentation-sensitive. Use a colon `:` for single-line blocks.
- **Nullability**: Use `Option<T>` (defined in `stdlib`).

---

## 3. Strict Coding Standards
- **Naming**: `UpperCamelCase` (Types/Traits), `snake_case` (Functions/Vars), `SCREAMING_SNAKE_CASE` (Constants).
- **Safety**: **NEVER** use `unwrap()` or `expect()` in library code. Propagate errors via `Result<T, MiriError>`.
- **Matching**: Exhaustive `match` is mandatory. Do not use `_` for domain-critical enums.
- **Standard Library Independence**: The compiler must NOT hardcode any standard library names or have specialized logic for them. Treat them like user code.
- **Separation of Concerns**: `struct` for data, `trait` for behavior. Avoid "God Objects".
- **Comments**: Keep comments up-to-date with the code. Remove obsolete comments. Don't add comments that mention phases or tasks from planning documents. Ensure copyright headers are present.

---

## 4. Testing & Verification (Mandatory)
Testing is the only way to prove your work is correct.

- **Integration Tests**: Located in `tests/integration/`. Use helpers in `tests/integration/utils.rs`:
    - `assert_runs(code)`: High-level success check.
    - `assert_runs_with_output(code, expected)`: Check for specific output.
    - `assert_compiler_error(code, "message")`: Test negative cases.
    - `assert_runtime_error(code, "message")`: Test runtime panics.
- **Running Tests**:
    - **CRITICAL**: The integration test binary is named `mod`, NOT `integration`. Always use `--test mod`.
    - Full suite: `cargo test --test mod`
    - Filter by name: `cargo test --test mod "test_name_filter"`
    - Example: `cargo test --test mod "test_list"` runs all tests whose name contains `test_list`
    - **WRONG**: `cargo test --test integration "..."` → error: no test target named `integration`
    - **CORRECT**: `cargo test --test mod "..."`
- **Verification Flow**:
    1. **`make format`**: MUST run after every change.
    2. **`make lint`**: Fix all clippy warnings.
    3. **`make build`**: Ensure both compiler and runtimes compile.
    4. **`make test`**: Run the full suite.

---

## 5. Workflow Best Practices for AI Agents
To work efficiently and hit fewer roadblocks:

1. **Research First**: Use `grep_search` to find examples of similar patterns (e.g., "how is `if` implemented in MIR?").
2. **Incremental Changes**: Complete one phase (e.g., Type Checker) with passing tests before moving to the next (e.g., MIR lowering).
3. **No Brute Force**: If you encounter a compilation error, analyze the `MiriError` or Rust error. Don't just `sed` the code.
4. **Update READMEs**: If you change a module's core logic, update its local `README.md`.
5. **Temporary Files**: Use `/tmp/` for scripts or backups.
6. **Follow-up Changes**: When you discover a follow-up that's not part of the current scope. always add it to the `notes/PLAN.md` file where appropriate. Don't just list them without recording.

---

## 6. Common Roadblocks & Troubleshooting
- **Linker Errors**: If you add a runtime intrinsic, ensure it's exported in `src/runtime/core` and correctly declared in Miri's STDLIB with the `runtime` keyword.
- **Type Checker Loops**: Ensure your inference logic has termination conditions, especially with generics.
- **`make format` diffs**: If `make format` fails, it usually means you forgot to run it. Run it and re-verify.
- **Unreachable Code**: The compiler pipeline is strict. If you add a variant to a MIR instruction, you MUST update all visitors and codegen.

---

By adhering to these rules, you maintain the zero-cost abstractions and representational safety required for the Miri compiler. Let's build the future of programming together.

