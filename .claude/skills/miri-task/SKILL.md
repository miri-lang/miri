---
name: miri-task
description: Fast single-agent end-to-end execution of a Miri compiler task — no subagents. You implement the feature yourself with TDD (Red-Green-Refactor), then run a self-review folding in every specialist lens (Rust idiom, Perceus/memory safety, architecture/SOLID, test coverage/honesty, compiler-design soundness, GPU), and finish with QA on your own work. Done ONLY when format, lint, build, the full test suite, and `make audit` are all green and the self-QA pass finds no open critical/major issue. Use for everyday features and fixes. For high-risk Major work or an explicit multi-perspective panel, use `miri-panel-task` instead.
---

# Miri task — single-agent fast path

**You do the whole task yourself, in the main thread. No subagents.** This is the fast path: one focused engineer who implements, reviews their own work through every specialist lens, QAs it, and refuses to declare done until the gate is green. It exists because delegating everyday features to a subagent panel is slow and the subagents over-report progress, miss real failures, and run out of context. You keep the context; you own the result.

**Binding standard: `PRINCIPLES.md` at the repo root.** Read it before writing code. Also honor `AGENTS.md`.

If the task is genuinely high-risk (a new `MirInstruction`/`Place`/terminator variant, a runtime ABI change, new `unsafe`, a non-trivial Perceus change, a GPU lowering change, or a cross-layer redesign — PRINCIPLES.md §8.1) and you want independent eyes, stop and tell the user to run `miri-panel-task` instead. Otherwise proceed solo.

## Explore via the code graph FIRST (do not read file-by-file)

Before any Grep/Glob/Read sweep, use the `code-review-graph` MCP tools — they are faster, cheaper, and give structural context a file scan cannot:

- `semantic_search_nodes` / `query_graph` to locate the closest analogous feature (how a similar operator lowers, how a sibling intrinsic is declared, how a method is intercepted) — instead of grepping.
- `get_impact_radius` + `get_affected_flows` to learn the blast radius **before** you touch anything (which visitors, call sites, tests are affected).
- `query_graph` pattern=`tests_for` to find existing coverage; `callers_of` / `callees_of` / `imports_of` to trace relationships.
- `get_review_context` for token-efficient source snippets when reviewing your own diff.

Fall back to Grep/Glob/Read only for what the graph does not cover. Reach analogous code through the graph, then read just the relevant span.

## Procedure

1. **Understand & challenge.** Restate the task and its acceptance criteria. If a milestone/plan-file/free-form arg was given, quote the deliverables back. Ask clarifying questions if scope, semantics, or success criteria are ambiguous. Challenge the request where it makes practical sense (wrong altitude, missing error path, conflicts with an existing invariant, simpler design available). Don't start coding until scope is confirmed.
2. **Map the change.** Use the graph to map scope to the pipeline (lexer → parser → `ast/factory.rs` → type checker → `mir/lowering/` (intercepts in `control_flow.rs`) → `mir/optimization/perceus.rs` → `codegen/cranelift/` → `runtime/{core,gpu}/` → `stdlib/**/*.mi`). Name any new files and confirm naming against the closest analog. Run `get_impact_radius` so you know every visitor/call site/test you must update in the same pass — don't discover them one breakage at a time (blast-radius first, per AGENTS.md §5).
3. **Implement with TDD, gated (MANDATORY — never skipped).** Per acceptance criterion:
   - **RED** — write the failing test first in `tests/integration/` (helpers: `assert_runs`, `assert_runs_with_output`, `assert_compiler_error`, `assert_runtime_error`, `assert_runtime_crash`). Run `cargo test --test mod "name"`. Confirm it fails for the *right* reason. If it passes immediately, the test is wrong.
   - **GREEN** — minimum code to pass. No speculative generality, no drive-by refactors.
   - **REFACTOR** — functions ≤ 80 lines (default ≤ 40), verbs for fns / nouns for types / predicates for bools, no duplication, exhaustive matches (no `_ =>` over Miri enums). Re-run after each step; revert any step that reddens the suite.
   - Cover the **error path** (`assert_compiler_error` / `assert_runtime_error`) — happy-path-only is incomplete. Stdlib tests mirror the source path under `tests/stdlib/**`; **never** `panic(...)` in `src/stdlib/**`.
4. **Self-review — fold in every specialist lens (do this on your own diff).** Walk each axis and fix what you find as you go:
   - **Compiler design soundness** — is a new IR/`Place`/terminator variant the right abstraction or a special-case? Is logic at the right layer (intercept vs class-method mangling vs runtime intrinsic)? Does it compose with existing lowering? Generic/value-generic handling without signature collisions?
   - **Visitor completeness** (§5.4) — new MIR/`Place` variant? `grep`/graph every match arm and visitor (`perceus.rs`, codegen, analyses) and update all. No `_ =>` masking a gap.
   - **Perceus / memory safety** (§5.1) — new managed temp, field projection, or method-dispatch intercept? `Copy` of a managed `Place` with empty projection gets IncRef; field-projected copies do NOT (guard `emit_temp_drop` on `projection.is_empty()`). Missed IncRef = UAF; spurious DecRef = double-free.
   - **Runtime/stdlib ABI as a trust boundary** (§5.2) — new intrinsic = three coordinated edits: export in `src/runtime/{core,gpu}/`, declare with the `runtime` keyword in the right `.mi`, rebuild (`cd src/runtime/core && cargo build --release`). Rust signature MUST match the Cranelift ABI for the declared param types (a width/pointer mismatch is corruption). Check `out`-param stack-slot copy-in/copy-out and `#[repr(C)]` layout.
   - **Bounds / overflow / hostile input** — index validated before the runtime touches the buffer; size/offset arithmetic can't wrap (`checked_*`/`saturating_*`); no `unwrap()`/`expect()`/`panic!` reachable from a crafted `.mi` source (that's a DoS — propagate via `Result<T, MiriError>`).
   - **Stdlib independence** (§1.1, §5.3) — never hardcode a stdlib type name (`"List"`, `"Set"`, …) in compiler dispatch; reach types via the type table. Highest-priority architecture rule.
   - **Architecture / SOLID** (§1–§2) — layer direction (no codegen types leaking into mir/type_checker), SRP (no `foo_and_bar`, no God object), OCP (`if backend == "..."` only in the dispatcher), real DRY duplication.
   - **Rust idiom & perf** (§3, §6) — no needless `clone()`/`to_string()` on managed/hot-path values, iterator chains over manual index loops, `?` over long-hand `match`, no avoidable O(n²) or hashing in tight loops, `&str`/`impl Iterator` returns where they save callers a copy.
   - **GPU** (only if WGSL / `src/runtime/gpu/` / residency / `gpu for|fn|let|var` is touched) — upload/readback byte counts vs buffer size, `GpuLaunchDesc` field widths in lockstep, dispatch grid vs the `SwitchInt` bounds guard, scalar-width portability and feature gating.
5. **Mechanical-transform safety.** After any dedent, `sed`/`perl`, `git checkout`, mass import removal, or bulk rename: re-read the touched files and rebuild. Confirm no string literal was broken, no trailing code lost, and no load-bearing import removed (a removed import can break transitive resolution and redden unrelated tests). Scope mechanical edits narrowly to the exact symbol/type being changed — broad sweeps cause whack-a-mole regressions.
6. **Adversarial self-QA.** Switch hats and try to break your own work. For each suspect path write a minimal `.mi` snippet you predict fails (empty collection, single element, nested generics, boundary index, overflow input, multiple moves, mixed residency). Run it. Audit your own tests for green-washing: an `assert_runs(...)` with no output/state assertion proves compilation not behavior; a test that was green before the feature existed proves nothing; a name describing implementation (`test_lower_call_intercept`) not behavior (`test_list_push_extends_length`) is a finding. Add the missing assertions/edge cases.
7. **Run the gate yourself — report exact counts.** In order: `make format` (empty diff) → `make lint` (clean) → `make build` → `make test` (`cargo test --test mod`, capture exact pass/fail/ignored) → `make audit` (clean for touched files: unwrap/expect, stdlib-name leaks, `_ =>` over Miri enums, oversized functions, banners, comment rot). **Do not infer success — read the actual output.** If any earlier subagent or note called a failure "pre-existing" or "out of scope", re-run that test yourself before trusting it.
8. **Loop tight.** Fix → re-run only what the fix touched (`make audit` + the affected tests always; full suite before declaring done). If the same root cause survives three attempts, stop and surface it to the user — don't churn.
9. **Docs / plan.** If a module's core logic changed, update its local `README.md`. If scope came from a plan file, mark items done. Record out-of-scope discoveries as `notes/PLAN.md` follow-ups (and TODO comments with context at the code site) — never silently widen scope.
10. **Final report** (format below).

## Final report format

```
# Miri Task — <task>
Status: DONE | NOT DONE (blockers open)
Scope delivered: <bullets>
Gate: format <clean> | lint <clean> | build <clean> | test <was N → now M passing / K ignored> | audit <clean>

## Implementation
<diff summary + RED/GREEN/REFACTOR log per criterion>

## Self-review (lenses applied)
Design <ok/notes> · Visitors <ok> · Perceus <ok> · ABI <ok> · Bounds <ok> · Stdlib-indep <ok> · Arch/SOLID <ok> · Rust <ok> · GPU <ok/N/A>

## Self-QA
<edge cases exercised + any green-washing fixed>

## Follow-ups recorded but NOT done (in notes/PLAN.md)
```

## Hard rules

- **Done only when format, lint, build, the full `cargo test --test mod` suite, and `make audit` are all green, and the self-QA pass leaves no open critical/major.** Run the gate yourself and report exact counts — never claim DONE on inference.
- **Never** skip the TDD RED/GREEN/REFACTOR gate per criterion.
- **Never** use `unwrap()`/`expect()`/`panic!` in library code — propagate via `Result<T, MiriError>`. Never `panic(...)` in `src/stdlib/**`.
- **Never** hardcode a stdlib type name in compiler dispatch. Always `cargo test --test mod` — never `--test integration`.
- **Never** widen scope beyond what was confirmed — record discoveries as follow-ups.
- **Never commit, stage, push, or touch git** (`git add`/`commit`/`push`/`stash`, branch creation, rebase). Leave all changes in the working tree for the user to review and commit.
- If the change trips a §8.1 Major-risk trigger and warrants independent review, recommend `miri-panel-task`.
