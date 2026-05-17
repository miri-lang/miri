# Miri Engineering Principles

This document is the **binding standard** for every change to the Miri compiler. It is referenced by `AGENTS.md`, the `miri-task` and `miri-audit` skills, and the `miri-reviewer` agent. If something in this file conflicts with a tutorial, a Stack Overflow answer, or your prior habit, this file wins.

The goal is not to recite Uncle Bob. The goal is to make the rules **operational**: every principle below has a *trigger* (when it applies), a *check* (how you verify it), and a *Miri-specific example*.

---

## 0. Reading guide

- **MUST / NEVER** — non-negotiable. A change violating these is rejected at review.
- **SHOULD** — strong default. Deviation requires a one-line justification in the PR description.
- **CONSIDER** — judgment call. Document the decision when the answer is non-obvious.

Every section ends with a **Self-check** block. The `miri-audit` skill grades a target against these.

---

## 1. Clean Architecture: the pipeline is the architecture

Miri's architecture is a *pipeline of pure transformations* with an inner core (semantics) and outer rings (text, machine code, OS). The **dependency rule** is non-negotiable: dependencies point inward.

```
  ┌──────────────────────────────────────────┐
  │ outer  ─►  text ↔ Lexer ↔ Parser         │
  │         ─►  AST                          │
  │   inner ►   Type Checker ◄─ stdlib (.mi) │
  │         ─►  MIR + optimization (Perceus) │
  │         ─►  Codegen (Cranelift / LLVM)   │
  │ outer  ─►  Runtime FFI ↔ OS              │
  └──────────────────────────────────────────┘
```

### 1.1 Layer rules (MUST)

- **Lexer** knows only bytes → tokens. It does not know about `Type`, `Mir`, or stdlib names.
- **Parser** knows only tokens → AST. It does not know about types, MIR, or codegen.
- **Type checker** knows AST + the user's stdlib `.mi` files. It does **not** import codegen, MIR optimization, or runtime symbols.
- **MIR** knows AST + types. It does **not** know about Cranelift `Value`, LLVM `IntPredicate`, or OS calls.
- **Codegen** knows MIR + a backend SDK. It does **not** mutate AST, modify the type table, or re-parse source.
- **Runtime** is an *independent crate*. It does not link the compiler. The compiler talks to it only through the FFI declared in stdlib `.mi` files with the `runtime` keyword.
- **Stdlib (`.mi` files)** is treated as user code. The compiler MUST NOT hardcode any stdlib type name, method name, or module path in branching logic. (Special-casing `List`, `Set`, `Option`, etc. is the most common architectural sin in this repo — flag and refuse.)

### 1.2 Cross-layer communication (MUST)

- Inter-layer types live in the inner layer and are *re-exported* outward. Not the other way around.
- A struct defined in `mir/` MUST NOT contain a field of a type defined in `codegen/`.
- Errors flow outward as `MiriError` (typed) — never as `String`, never as `panic!`, never as a Cranelift error leaked to the parser.

### 1.3 Self-check

- [ ] No `use crate::codegen::*` inside `src/type_checker/` or `src/mir/`.
- [ ] No stdlib type name appears as a string literal in compiler control flow (search: `"List"`, `"Set"`, `"Option"`, `"Map"`, `"String"` inside `src/` excluding `src/stdlib/`).
- [ ] No `panic!` / `unwrap` / `expect` in library code (`src/` excluding tests and `main.rs` argument parsing).
- [ ] Every new MIR variant is updated in **every** visitor (`perceus.rs`, codegen, analysis passes) — no wildcard arm swallows it.

---

## 2. SOLID

Each principle below is interpreted *for compiler infrastructure*, not for hypothetical OOP business apps.

### 2.1 Single Responsibility (SRP) — MUST

A module, struct, or function has one reason to change. In a compiler:

- A `lower_*` function lowers one AST construct to MIR. It does not also type-check or emit code.
- A `check_*` function validates one type rule. It does not mutate the MIR.
- A struct holds the data for one concept (`StructDefinition`, `MirInstruction::Call`, `ReferenceCount`). If two unrelated bools live in the same struct only because they're "always together at the call site", split them.

**Trigger to split**: the function name contains *and*, or the file's table of contents reads like a grab-bag.

**Check**: open the file. Can you summarize its purpose in **one sentence** without "and"? If not, split it.

### 2.2 Open/Closed (OCP) — SHOULD

Pipeline stages are *open* for new AST/MIR/Type variants and *closed* against re-shaping the pipeline orchestrator (`src/pipeline.rs`).

- Adding a new MIR instruction adds a variant + visitor arms — the pipeline driver does not change.
- Adding a new backend (LLVM, future GPU) adds a module under `codegen/`. The codegen *trait* (or dispatcher) is the contract; the driver does not branch on backend identity.

**Anti-pattern**: `if backend == "cranelift" { … } else if backend == "llvm" { … }` inside generic logic. Push the branch into a trait method.

### 2.3 Liskov Substitution (LSP) — MUST (for traits) / informational (for class inheritance)

Trait implementors MUST honor the trait's contract. A `Visitor` impl MUST visit every node the trait promises to visit. A `MiriError` variant MUST preserve span information if the trait says it does.

For Miri language class inheritance: see [Memory: class method dispatch](#) — codegen does not yet resolve inherited methods correctly. Document inherited-method limitations rather than papering over them.

### 2.4 Interface Segregation (ISP) — SHOULD

A consumer should not depend on methods it does not use.

- A type checker analysis that only needs `class_definitions` should not take the whole `TypeContext` by mutable reference. Pass the slice it needs.
- A MIR pass that only reads MIR should accept `&Body`, not `&mut Body`.

### 2.5 Dependency Inversion (DIP) — SHOULD

High-level modules (`pipeline.rs`, `Compiler`) depend on **abstractions** (traits, enums of backends, `Reporter` trait for diagnostics), not concrete implementations.

- Diagnostics: pipeline emits via a trait so tests can swap in an in-memory collector.
- Filesystem: the loader is a trait so a fixture-driven test can feed virtual files.

**Trigger**: a unit test that needs to mock something *cannot* mock it because the dependency is hard-wired. Refactor toward DIP.

### 2.6 Self-check

- [ ] Each function has one verb in its name (`lower_call`, not `lower_and_check_call`).
- [ ] No new `if backend ==` in the pipeline driver.
- [ ] No public function takes `&mut Everything` when it only mutates one field.
- [ ] Tests can construct subjects without booting the full pipeline.

---

## 3. Clean Code

### 3.1 Functions (MUST)

- **Small**. Default ceiling: **40 lines** body. Hard ceiling: **80 lines**. Above 80 → split or justify in the PR.
- **One level of abstraction per function.** Don't mix iterating bytes and emitting MIR in the same function.
- **Argument count ≤ 4.** Above → bundle into a struct (e.g. `CallSiteCtx`).
- **No flag arguments.** A `bool` argument that changes behavior is two functions.
- **No output arguments** other than the explicit Miri `out` parameter convention (which is an *intentional* language feature, not a code smell).
- **Return early.** Guard clauses over nested `if let Some`.

### 3.2 Naming (MUST)

- Functions are **verbs**: `parse_expression`, `lower_call`, `infer_type`, `emit_drop`.
  - **Exception (intentional)**: recursive-descent parser rules in `src/parser/` and AST node constructors in `src/ast/factory.rs` are named after the **grammar non-terminal / AST node they produce** (e.g. `fn identifier()`, `fn expression()`, `fn expr(...)`, `fn class_statement(...)`). The function name *is* the produced noun. Do not rename these to `parse_*` / `make_*` / `build_*` — the convention is a deliberate readability choice that mirrors the grammar. New parser/factory functions follow the same noun convention.
- Types and traits are **nouns**: `MirInstruction`, `Place`, `Visitor`.
- Booleans are **predicates**: `is_resource`, `has_drop`, `needs_out_pointer`.
- Constants are **SCREAMING_SNAKE_CASE**.
- **No abbreviations** unless they're the term of art (`mir`, `rc`, `ast`, `ffi`). Avoid `cfg`, `ctx`, `dst`, `src` when the full word fits — but if the surrounding code uses the abbreviation consistently, follow it.
- **No mental mapping**. `t` is allowed for a single-line closure. It is not allowed for a function-scope variable.

### 3.3 Comments (MUST)

- Default: **no comment**. Code should explain itself.
- Allowed: a comment explaining a *non-obvious* invariant, a workaround for an upstream bug, a Cranelift quirk, or a Perceus subtlety.
- **NEVER** write a comment that:
  - Restates what the code does (`// increment x`).
  - References a planning doc (`// per §7.4`, `// task 1.5`, `// milestone 12`).
  - Marks ownership (`// added by …`).
  - Is a section banner (`// ── parsing helpers ──`). If a file needs banners, split it.
- Doc-comments (`///`) on public items: yes, when the contract is not obvious from the signature.

### 3.4 Error handling (MUST)

- **NEVER** `unwrap()` or `expect()` in `src/` (library code). Allowed in `tests/`, allowed in `main.rs` for unrecoverable startup failures with a user-friendly message.
- All compiler errors flow as `MiriError` (or the module-local error type that converts into `MiriError`).
- A `?` is the only acceptable shortcut. `let Ok(x) = … else { unreachable!() }` is a panic with extra steps — refuse.
- Runtime intrinsics that can fail return a `MiriResult` or `Option` to Miri code. They never `abort()` silently.

### 3.5 Matching (MUST)

- **Exhaustive `match`** is mandatory for any enum the compiler defines.
- **NEVER** use `_ =>` to cover domain-critical enums (`MirInstruction`, `TypeKind`, `Place::Projection`). The compiler must fail to build when a new variant is added.
- `_ =>` is acceptable only for *open* enums imported from external crates (e.g. `cranelift_codegen::ir::Opcode`) where exhaustiveness is impossible.

### 3.6 Classes & data (SHOULD)

- **Data classes are data.** No methods that reach into another module's state.
- **Behavior lives in traits or free functions.** A `MirInstruction` doesn't lower itself; a `Lowerer` lowers it. (Exception: small data-local helpers like `Place::is_managed()`.)
- **No God objects.** A struct with > 10 fields needs a justification or a split.

### 3.7 Self-check

- [ ] No function in the diff is > 80 lines.
- [ ] No new `unwrap()` / `expect()` in `src/`.
- [ ] No new `_ =>` arm for a Miri-defined enum.
- [ ] No new comment restates code, references a plan, or labels a section.
- [ ] No new file imports a stdlib type name as a string literal.

---

## 4. TDD: Red-Green-Refactor is mandatory

Tests are not a deliverable bolted on after the work. They are the **specification of the work**.

### 4.1 The cycle (MUST)

For every behavior change:

1. **RED** — write a test that captures the new behavior. Run it. Confirm it fails for the *right* reason (not for a typo, not because the test infrastructure broke). Capture the failure message.
2. **GREEN** — write the *minimum* code that makes the test pass. No speculative generality. No "while I'm here" refactors.
3. **REFACTOR** — with tests green, clean up names, extract functions, collapse duplication. Re-run tests after every refactor step.
4. **REPEAT** — next subtask.

The `miri-task` skill gates this cycle. A task is **not done** until each acceptance criterion went through all three phases.

### 4.2 What counts as a test (MUST)

- **Integration tests** under `tests/integration/` are the primary suite. Use the helpers (`assert_runs`, `assert_runs_with_output`, `assert_compiler_error`, `assert_runtime_error`, `assert_runtime_crash`).
- **Unit tests** under `#[cfg(test)] mod tests` are appropriate for pure-function logic (`Place::projection` checks, error formatting, etc.).
- A change *only verified by manual `cargo run`* is **not tested**. Add the test before declaring done.

### 4.3 Test discipline (MUST)

- Every new public function and every changed branch has at least one test covering it.
- Every error path is tested via `assert_compiler_error` or `assert_runtime_error`. "Happy path only" is rejected.
- Test names describe the behavior, not the implementation: `test_list_push_extends_length`, not `test_list_lower_call_intercept`.
- Test files mirror `src/`. Where stdlib lives, tests mirror the same directory structure (`tests/stdlib/collections/list.mi` for `src/stdlib/collections/list.mi`).
- **NEVER** call `panic(...)` inside `src/stdlib/**/*.mi`. Failure must surface via `T?` or `Result<T, E>`.

### 4.4 Test pyramid for Miri

- **Base**: small `.mi` snippets exercising one feature, asserted via the helpers.
- **Middle**: cross-feature interaction tests (generics + collections, Perceus + nested moves).
- **Top**: end-to-end programs in `tests/integration/programs/` that compile + run and assert stdout.

Skewed pyramids (mostly E2E, no unit) make regressions hard to localize.

### 4.5 Self-check

- [ ] Every changed file has either a corresponding test file or a justification.
- [ ] Every new public function is covered by ≥ 1 test.
- [ ] Every error path is covered.
- [ ] No `panic(...)` inside `src/stdlib/**`.

---

## 5. Miri-specific invariants

These are repeat offenders. They are *not optional* even when the rest of the change is clean.

### 5.1 Perceus reference counting

- A `Copy` of a managed `Place` with an empty projection gets an `IncRef`. With a non-empty projection (`obj.field`), Perceus does **not** IncRef — the call-site code that lifts the field must guard `emit_temp_drop` with `projection.is_empty()`.
- A new temporary holding a managed value gets a `StorageDead` that translates to `DecRef`.
- Method-dispatch intercepts in `src/mir/lowering/control_flow.rs` MUST follow the established pattern (`length`, `element_at`, `push`, `set`, `insert`).
- Use-after-move checking is layered: resource types are consumed at *every* scope; managed (non-resource) types are consumed only at the top level. Do not collapse this distinction.

### 5.2 Runtime / stdlib alignment

- A new intrinsic requires three coordinated edits:
  1. Rust function exported in `src/runtime/core/`.
  2. `runtime "core" fn` declaration in the right `src/stdlib/**/*.mi` file.
  3. Rebuild: `cd src/runtime/core && cargo build --release`.
- The Rust signature MUST match the Cranelift ABI for the parameter types declared in the `.mi` file.
- `out` parameters: scalars use copy-in/copy-out via a caller stack slot; managed types are pointers and need no ABI change.

### 5.3 Stdlib independence

The compiler treats `system.collections.List` exactly the same as a user struct `MyThing`. No paths in `src/` may check for the string `"List"` or `"Set"` or `"Option"` in dispatch logic. This is the single highest-priority architectural rule.

### 5.4 Exhaustive matching across visitors

Adding a `MirInstruction` variant changes the contract of every visitor. Use `grep -rn "MirInstruction::"` to enumerate every site. Any `_ =>` you find that handles `MirInstruction` is a bug — convert it to an exhaustive match.

### 5.5 Self-check

- [ ] All Perceus-touching changes pass the projection-guard pattern.
- [ ] Runtime rebuilt after any `src/runtime/core/` change.
- [ ] `grep -rn '"List"\|"Set"\|"Option"\|"Map"\|"String"' src/ | grep -v 'src/stdlib'` returns no dispatch-logic hits.
- [ ] No `_ =>` arms over `MirInstruction`, `TypeKind`, or `PlaceElem`.

---

## 6. Smells & antipatterns

Auto-flag in audit if seen:

| Smell | Why it's bad | Fix |
|------|--------------|-----|
| Function > 80 lines | Hides a missing abstraction. | Extract by section. |
| `fn foo_and_bar()` | SRP violation. | Split into `foo()` + `bar()`. |
| `unwrap()` / `expect()` in `src/` | Production panic. | Propagate via `MiriError`. |
| `_ =>` over a Miri enum | Silences future contracts. | Exhaustive match. |
| `if backend == "..."` outside the dispatcher | OCP violation. | Push into trait method. |
| `"List".to_string()` in compiler code | Stdlib coupling. | Reach the type via the type table. |
| `// per §X.Y` / `// task N` | Comment rot. | Delete; trust the PR description. |
| `// ── topic ──` banner | File too big / SRP violation. | Split file. |
| `let x = …; let y = x.clone();` for a managed type, with no Perceus consideration | UAF or double-free risk. | Audit IncRef/DecRef paths. |
| A test that only calls `assert_runs(...)` with no output assertion | Confirms compilation, not behavior. | Add output / state assertion. |
| Public function with no test | Untested contract. | Add test before merge. |
| `_var` rename to silence unused warnings | Hides dead code. | Delete the binding (or the parameter, if safe). |
| Section comment "removed X" left in source | Comment rot. | Delete it. |

---

## 7. Self-audit checklist (used by `miri-audit`)

Score each dimension **A / B / C / F**:

1. **Architecture**: layer boundaries, dependency direction, stdlib independence.
2. **SOLID**: SRP, OCP, LSP, ISP, DIP.
3. **Clean Code**: function size, naming, comments, error handling, exhaustiveness.
4. **TDD**: test-first, coverage of new branches, error-path tests.
5. **Miri invariants**: Perceus, runtime/stdlib alignment, exhaustive visitors.
6. **Smells**: count and severity.

An overall grade lower than **B** in any dimension requires a fix list before the change ships.

---

## 8. How this document is enforced

- **Workflow**: `miri-task` skill drives Red-Green-Refactor and runs `make audit` at the end.
- **Review**: `miri-reviewer` agent checks every diff against §1–§5.
- **On-demand audit**: `miri-audit` skill grades any path/branch against §7 and proposes diffs (report-only by default).
- **Mechanical**: `clippy.toml` thresholds + `Cargo.toml` `[lints.clippy]` enforce function size, argument count, complexity, and `unwrap_used` for `src/`. `make lint` is part of every verification gate.
- **Memory**: `MEMORY.md` references this file so future sessions inherit the standard.

When in doubt, optimize for the next reader. The next reader is often you.
