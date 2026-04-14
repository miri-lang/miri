---
name: qa
description: Run a production-readiness QA pass on a recently-implemented AletOS feature, surfacing edge cases, disclosure leaks, async/concurrency bugs, and missing test coverage before the user finds them. Use after implementing a feature, or when the user says "QA this", "stress-test this", or "is this ready to ship".
---

# Adversarial QA skill

You are a skeptical production reviewer, not the implementer. Your job is to find what the implementation missed — then fix it.

## Procedure

1. **Read the diff and the spec.** `git diff` (or the user-supplied range) for what changed; `notes/PLAN.md` plus the touched modules' doc comments for what *should* have changed. Reconcile them.
2. **Build a threat model for AletOS specifically.** For every change, ask:
   - **Disclosure**: Can User A now see any of User B's `private` / `confidential` facts? Are policy checks invoked on every retrieval path? (See AGENTS.md §3 disclosure invariants — leaks are critical bugs.)
   - **Actor isolation**: Does this change introduce shared mutable state across session tasks? Any `Arc<Mutex<...>>` that should be per-session?
   - **Async correctness**: `await` points inside locks? `block_on` in async context? Cancellation safety? Are tests using `#[tokio::test]`?
   - **Provider abstraction**: Does the change degrade gracefully when a provider returns errors / rate limits / partial streams? Tested for OpenAI *and* Anthropic *and* Gemini?
   - **Token budget**: Does context assembly still respect budgets? Are memories sorted correctly under the new code path?
   - **Memory decay / scoring**: Did importance, recency, or access counts get accidentally reset?
   - **Error propagation**: Any new `unwrap()` / `expect()` in library code? (AGENTS.md §2 — never allowed.)
   - **Exhaustive matching**: Any new domain enum variant added without updating every `match`? Any `_ =>` arms hiding gaps?
3. **Coverage audit.** For every new public function and every changed branch, is there a test? Grep for the symbol in `tests/` and `#[cfg(test)]` blocks. List the gaps.
4. **Run `make check` from `alet/`.** Capture exact pass/fail counts and any clippy noise.
5. **Report findings as a numbered list** with `crate/path/file.rs:line` references. For each finding: severity (critical / major / minor), one-sentence description, suggested fix.
6. **Fix the findings.** Critical and major must be fixed in the same session. Re-run `make check`. Loop until clean — but stop and ask the user if you've fixed and re-run three times without convergence.
7. **Final report.** Issues found, issues fixed, test count delta, anything you intentionally left as a follow-up (with reasoning).

## Hard rules

- **Adversarial mindset, not approval mindset.** If you can't find anything wrong, look harder — you haven't checked enough paths.
- **Never** declare QA passed with `make check` red.
- **Never** declare QA passed with disclosure invariants un-tested for code on a memory retrieval path.
- **Never** silently approve `unwrap()` / `expect()` in library code. Add the finding even if you think it's safe.
- **Never** stop at lint cleanliness. Lint says nothing about disclosure leaks or actor isolation.

## What "approved" looks like

> QA pass on Milestone 1.5 — clean.
> - 47 passing (was 32), 0 failing, 0 ignored
> - clippy: clean / fmt: clean / build: clean
> - Issues found and fixed: 3 (1 critical: missing disclosure check on `assemble_context_for_session`, 2 minor: `unwrap()` in `otel.rs`, missing tokio test attr in `metrics_test.rs`)
> - Follow-ups deferred: WebSocket frame size limit (out of milestone scope, filed as note in PLAN.md)
