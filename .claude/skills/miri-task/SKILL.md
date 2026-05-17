---
name: miri-task
description: Execute a Miri compiler task end-to-end with TDD, full verification, and an honest pass/fail report. Use when the user references a milestone, a plan file, or provides a task description directly.
gemini-model: pro
---

# Miri task execution skill

You are executing a task on the Miri compiler. Your standard is production-grade compiler infrastructure — correctness, exhaustive matching, no `unwrap()` in library code, and zero regressions.

**Binding standard: `PRINCIPLES.md` at the repo root.** Read it before you start. It encodes Clean Architecture (layer rules, stdlib independence), SOLID, Clean Code (function size, naming, comments, error handling), TDD discipline, and Miri-specific invariants (Perceus, runtime/stdlib alignment, exhaustive visitors). Every step below is graded against PRINCIPLES.md.

## Inputs

Arguments after the slash command name the task. Accepted forms:
- A milestone reference (e.g. `/miri-task 1.5`, `/miri-task "Phase 2"`) — locate it in a plan file.
- A file path (e.g. `/miri-task notes/tasks/feature.md`) — read that file for scope and acceptance criteria.
- Free-form text (e.g. `/miri-task "Implement Set.union method"`) — treat the text itself as the spec.

If no argument is given, ask the user what to implement — do not guess or invent scope.

## Procedure

1. **Establish scope.** Depending on input:
   - *Milestone reference*: Open the referenced plan file (ask the user if you don't know which), locate the milestone, and quote its deliverables back to the user.
   - *File path*: Read the file and quote its acceptance criteria back to the user.
   - *Free-form text*: Restate your understanding of the task and ask for confirmation before writing any code.
2. **Map scope to modules.** Identify which parts of the compiler pipeline are touched (lexer, parser, type checker, MIR lowering, codegen, runtime, stdlib). Note any new files needed and confirm naming before creating them.
3. **Audit existing patterns first.** Before writing code, `Grep` for similar features already implemented (e.g. how an analogous operator is lowered to MIR, how a similar runtime intrinsic is declared in stdlib). Mirror those conventions exactly — see AGENTS.md §3.
4. **Break the work into tasks with TaskCreate.** One task per acceptance criterion. Mark `in_progress` / `completed` as you go.
5. **TDD per subtask — Red / Green / Refactor (MANDATORY, gated).** No subtask is "done" until all three phases are complete *and* logged.
   - **RED** — write the test first in `tests/integration/` using the helpers (`assert_runs`, `assert_runs_with_output`, `assert_compiler_error`, `assert_runtime_error`, `assert_runtime_crash`). Run it: `cargo test --test mod "test_name"`. **Capture the failure message** and confirm it is failing for the *right* reason (the behavior under test, not a typo, missing import, or broken fixture). If the test passes immediately, the test is wrong — fix the test, not the code.
   - **GREEN** — write the **minimum** code that turns the test green. No speculative generality. No "while I'm here" refactors. No new abstraction unless the test demands it. Re-run the same test command and confirm green.
   - **REFACTOR** — with the suite green, clean up: shorten functions to ≤ 80 lines (default ≤ 40), apply naming rules (verbs for fns, nouns for types, predicates for bools), remove duplication, exhaustive-match any new variants, ensure no `_ =>` over Miri enums. Re-run tests after each refactor step. If a refactor breaks a test, *revert that step* and try a different approach.
   - Cover the **error path** too: a feature with no `assert_compiler_error` / `assert_runtime_error` test is incomplete.
   - For stdlib changes: tests live at the mirrored path under `tests/stdlib/**` (e.g. `tests/stdlib/collections/list.mi` for `src/stdlib/collections/list.mi`). No `panic(...)` in `src/stdlib/**`.
6. **Find-all-sites discipline.** If you add a MIR instruction variant, a new error type, or change a shared trait, `Grep` for every match arm, every `match`, every call site across the workspace and update them. Never leave the tree with unhandled variants.
7. **Perceus correctness check.** If your change touches object fields, method dispatch, or temporary copies, verify that Perceus RC accounting is correct — review `src/mir/optimization/perceus.rs` and the `is_place_managed` / `obj_op_is_copy` patterns documented in project memory.
8. **Runtime/stdlib alignment.** If you add a runtime intrinsic: export it in `src/runtime/core/`, declare it in the appropriate stdlib `.mi` file with the `runtime` keyword, and rebuild the runtime (`cd src/runtime/core && cargo build --release`).
9. **Final verification gate.** Before the completion message, run in order:
   - `make format` — must produce no diff.
   - `make lint` — must be clean (no clippy warnings).
   - `make build` — compiler and runtime must compile.
   - `make test` — full suite must pass.
   - `make audit` — mechanical sweeps from PRINCIPLES.md (no new `unwrap` in `src/`, no stdlib-name leaks, no section banners, no comment rot, no `_ =>` over Miri enums). Output must be clean *for the touched files*.
   - Report exact pass counts and `format: clean / lint: clean / build: clean / test: N passing / audit: clean`.
   - If anything is red, fix it. Do **not** declare done with regressions.

9.5. **Principle self-audit.** Before declaring done, grade the diff against the dimension lists in PRINCIPLES.md §1.3 / §2.6 / §3.7 / §4.5 / §5.5. If any dimension is below **B**, fix it now. Quote the principle being applied when you make the fix.
10. **Update documentation.** If the change affects a module's core logic, update its local `README.md`. If scope came from a plan file, mark the relevant items done.
11. **Summarize.** End with: scope delivered, test count delta (e.g. "47 → 53 passing"), any decisions the user should review, any follow-ups discovered but explicitly *not* done.

## Hard rules

- **Never** declare a task complete with `make test` red.
- **Never** declare a task complete without the RED / GREEN / REFACTOR log per subtask. Skipping a phase is the most common failure mode — do not.
- **Never** declare a task complete with `make audit` reporting new violations in touched files.
- **Never** use `unwrap()` or `expect()` in library code — propagate via `Result<T, MiriError>`.
- **Never** leave a new enum variant without updating every `match` that covers it.
- **Never** widen scope beyond what the user specified. If you discover something that ought to be done, write it down as a follow-up — don't silently expand.
- **Never** present options when the spec is unambiguous. Execute.
- **Never** hardcode standard library names in the compiler. Treat stdlib as user code.
- If `make test` fails three times on the same root cause, stop and ask the user — don't churn.
- Always use `cargo test --test mod` — **never** `--test integration` (that target does not exist).

## Example opening message

> Task: Implement `Set.union` method.
>
> Scope: Add `union(Set<T>) Set<T>` method to Set in the stdlib, a runtime intrinsic `miri_rt_set_union` in `src/runtime/core/`, MIR lowering intercept in `control_flow.rs`, and integration tests in `tests/integration/collections/set.rs`.
>
> Mirroring the existing `set_contains` / `Set.contains` pattern. Starting with the failing integration test now.
