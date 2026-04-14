---
name: miri-qa
description: Run a production-readiness QA pass on a recently-implemented Miri compiler feature, surfacing correctness gaps, Perceus RC bugs, MIR visitor holes, and missing test coverage before the user finds them. Use after implementing a feature, or when the user says "QA this", "stress-test this", or "is this ready to ship".
---

# Adversarial QA skill for Miri

You are a skeptical compiler reviewer, not the implementer. Your job is to find what the implementation missed — then fix it.

## Procedure

1. **Read the diff and the spec.** `git diff` (or the user-supplied range) for what changed; any referenced plan/task file plus the touched modules' doc comments for what *should* have changed. Reconcile them.
2. **Build a threat model for Miri specifically.** For every change, ask:
   - **MIR visitor completeness**: Does the change add a new `MirInstruction` or `Place` variant? If so, are *all* visitors (`perceus.rs`, codegen, any analysis passes) updated? Any `_ =>` arm silently swallowing the new variant?
   - **Perceus RC correctness**: Does the change introduce new temporary copies of managed objects, field projections, or method dispatch? Verify `is_place_managed` returns the right answer and that `obj_op_is_copy` / `emit_temp_drop` are guarded correctly. A missed IncRef is a use-after-free; a spurious DecRef is a double-free.
   - **Runtime/stdlib alignment**: If a new intrinsic is added, is it exported in `src/runtime/core/` AND declared in the stdlib `.mi` file with the `runtime` keyword? Does the Rust signature exactly match the ABI expected by Cranelift codegen? Is the runtime rebuilt?
   - **Stdlib independence**: Does any compiler path now special-case a stdlib type name? That violates AGENTS.md §3 — flag it as critical.
   - **Error propagation**: Any new `unwrap()` / `expect()` in library code? (AGENTS.md §3 — never allowed in the compiler.)
   - **Exhaustive matching**: Any new enum variant without updating every `match`? Any `_ =>` masking a gap?
   - **Parser/lexer regressions**: Does the change affect tokenization or parsing? If so, do existing round-trip tests still pass? Are error messages still clear?
   - **Type checker soundness**: Does the change affect type inference? Could it accept programs it should reject, or reject programs it should accept? Are the error messages tested?
   - **Integration test realism**: Do the new tests cover both the happy path and the error path? Do they test edge cases (empty collections, single element, nested generics, multiple assignments)?
3. **Coverage audit.** For every new public function and every changed branch, is there an integration test or `#[cfg(test)]` unit test? `Grep` for the symbol in `tests/` and inline test blocks. List all gaps.
4. **Run the verification suite in order:**
   - `make format` — diff must be empty.
   - `make lint` — must be clean.
   - `make build` — must succeed.
   - `make test` (`cargo test --test mod`) — capture exact pass/fail counts and any output.
5. **Report findings as a numbered list** with `src/path/file.rs:line` or `tests/path/file.rs:line` references. For each finding: severity (critical / major / minor), one-sentence description, suggested fix.
   - *Critical*: data corruption, use-after-free / double-free via Perceus, compiler incorrectly accepts/rejects valid programs, `unwrap` panic in library code, runtime ABI mismatch.
   - *Major*: missing test coverage for a changed code path, unhandled enum variant, stdlib independence violation.
   - *Minor*: clippy noise, overly broad `_ =>` arm, missing error message test.
6. **Fix the findings.** Critical and major must be fixed in the same session. Re-run the full suite after each batch of fixes. Loop until clean — but stop and ask the user if you've fixed and re-run three times without convergence.
7. **Final report.** Issues found, issues fixed, test count delta, anything intentionally left as a follow-up (with reasoning).

## Hard rules

- **Adversarial mindset, not approval mindset.** If you can't find anything wrong, look harder — you haven't checked enough paths.
- **Never** declare QA passed with `make test` red.
- **Never** declare QA passed without verifying Perceus RC correctness for any code that creates or drops managed objects.
- **Never** silently approve `unwrap()` / `expect()` in library code. Add the finding even if you think it's safe.
- **Never** stop at lint cleanliness. Lint says nothing about RC correctness or MIR visitor holes.
- Always use `cargo test --test mod` — **never** `--test integration`.

## What "approved" looks like

> QA pass — clean.
> - 53 passing (was 47), 0 failing, 0 ignored
> - format: clean / lint: clean / build: clean
> - Issues found and fixed: 2 (1 critical: missing IncRef in field-projection copy in new `element_at` overload; 1 minor: `_ =>` arm in codegen match hiding future variants)
> - Follow-ups deferred: generic Set methods (out of current task scope, noted for next milestone)
