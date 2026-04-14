---
name: milestone
description: Execute an AletOS milestone from notes/PLAN.md end-to-end with TDD, full make check verification, and an honest pass/fail report. Use when the user references a milestone number (e.g. "1.5", "2.1", "Phase 2") or says "do milestone X".
---

# Milestone execution skill

You are executing an AletOS milestone. Treat `notes/PLAN.md` as the source of truth for scope and acceptance criteria.

## Inputs

Arguments after the slash command name the milestone (e.g. `/milestone 1.5` or `/milestone "Observability Foundation"`). If no argument is given, ask which milestone — do not guess.

## Procedure

1. **Read the plan.** Open `alet/notes/PLAN.md` and locate the named milestone. Quote its bullet list of deliverables back to the user in your first message so it's clear what you're committing to.
2. **Map the scope to crates.** Identify which crates under `alet/crates/` will be touched. If the milestone introduces a new crate, note that and confirm naming with the user before scaffolding.
3. **Audit existing patterns first.** Before writing code, `Grep` the workspace for similar features (e.g. existing trait impls, error variants, axum routes, otel instrumentation) and pick one as your exemplar. Mirror its conventions — see AGENTS.md §3 "Match existing conventions".
4. **Break the work down with TaskCreate.** One task per acceptance criterion in the plan. Mark `in_progress` / `completed` as you go.
5. **TDD per subtask.**
   - Write a failing test first (`#[cfg(test)]` inline, or in `tests/` for cross-crate).
   - Run that test to confirm it fails for the *right* reason.
   - Implement the minimum to make it pass.
   - Run that test plus any obviously-related tests.
6. **Find-all-sites discipline.** If you change a trait, error type, or shared utility, grep the workspace for *every* call site and update them in the same change. Never leave the tree red.
7. **Final verification gate.** Before the completion message:
   - `cd alet && make check` must exit 0.
   - Report exact pass counts per crate, plus `clippy: clean / fmt: clean / build: clean`.
   - If anything is red, fix it. Do **not** declare done with regressions.
8. **Update the plan.** Mark the milestone's bullets as done in `notes/PLAN.md` (it's gitignored locally — that's fine). If acceptance criteria changed during implementation, update them with a one-line rationale.
9. **Summarize.** End with: scope delivered, test count delta (e.g. "32 → 47 passing"), any decisions the user should review, any follow-ups discovered but explicitly *not* done.

## Hard rules

- **Never** declare a milestone complete with `make check` red.
- **Never** widen scope beyond what `PLAN.md` lists for this milestone. If you discover something that ought to be done, write it down as a follow-up — don't silently expand.
- **Never** present options when the plan is unambiguous. Execute.
- **Never** swap targeted `cargo test -p ...` for the full `make check` in the final gate. They are not equivalent.
- If `make check` fails three times on the same root cause, stop and ask the user — don't churn.

## Example opening message

> Milestone 1.5 — Observability Foundation. From `notes/PLAN.md`:
> - Integrate OpenTelemetry across actor → memory → LLM → skill → response
> - LLM call logging (prompt/response, tokens, latency, model)
> - Metrics: p50/p95/p99 latency, token spend per user, memory growth, active sessions
>
> Plan: new `alet-observe` crate, wire it into `alet-kernel` and `alet-runtime`, add an axum middleware in `alet-api`. Mirroring tracing patterns already used in `alet-kernel/src/session.rs`. Starting with the OTEL exporter test now.
