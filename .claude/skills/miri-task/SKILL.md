---
name: miri-task
description: Execute a Miri compiler task end-to-end with TDD, full verification, and an honest pass/fail report. Use when the user references a milestone, a plan file, or provides a task description directly.
---

# Miri task execution skill

You are executing a task on the Miri compiler. Your standard is production-grade compiler infrastructure — correctness, exhaustive matching, no `unwrap()` in library code, and zero regressions.

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
5. **TDD per subtask.**
   - Write the integration test first in `tests/integration/` using the helpers (`assert_runs`, `assert_runs_with_output`, `assert_compiler_error`, `assert_runtime_error`).
   - Run it to confirm it fails for the *right* reason: `cargo test --test mod "test_name"`.
   - Implement the minimum code to make it pass.
   - Re-run to confirm green.
6. **Find-all-sites discipline.** If you add a MIR instruction variant, a new error type, or change a shared trait, `Grep` for every match arm, every `match`, every call site across the workspace and update them. Never leave the tree with unhandled variants.
7. **Perceus correctness check.** If your change touches object fields, method dispatch, or temporary copies, verify that Perceus RC accounting is correct — review `src/mir/optimization/perceus.rs` and the `is_place_managed` / `obj_op_is_copy` patterns documented in project memory.
8. **Runtime/stdlib alignment.** If you add a runtime intrinsic: export it in `src/runtime/core/`, declare it in the appropriate stdlib `.mi` file with the `runtime` keyword, and rebuild the runtime (`cd src/runtime/core && cargo build --release`).
9. **Final verification gate.** Before the completion message, run in order:
   - `make format` — must produce no diff.
   - `make lint` — must be clean (no clippy warnings).
   - `make build` — compiler and runtime must compile.
   - `make test` — full suite must pass.
   - Report exact pass counts and `format: clean / lint: clean / build: clean / test: N passing`.
   - If anything is red, fix it. Do **not** declare done with regressions.
10. **Update documentation.** If the change affects a module's core logic, update its local `README.md`. If scope came from a plan file, mark the relevant items done.
11. **Summarize.** End with: scope delivered, test count delta (e.g. "47 → 53 passing"), any decisions the user should review, any follow-ups discovered but explicitly *not* done.

## Hard rules

- **Never** declare a task complete with `make test` red.
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
