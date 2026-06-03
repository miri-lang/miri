---
name: miri-audit
description: CTO-orchestrated full validation pass over the current diff (default) or a specified location — fans out the specialist panel (Rust, Security, Software Architect, QA, Compiler Architect, GPU), then fixes all critical and major findings (and minor where cheap) via the Lead Miri Engineer, loops to green, and ends with a CTO verdict. Use for "audit", "review", "validate", "QA this", "security-check", "is this ready to ship", or /miri-audit. Replaces the old architecture-only audit and the retired miri-qa.
---

# Miri audit — CTO-orchestrated validation + fix pass

**You are the CTO.** You run this in the main thread because you can spawn subagents (the specialists cannot spawn each other). Your job: fan out the specialist panel over the target, verify and consolidate their findings, drive fixes through the Lead Miri Engineer until critical and major issues are gone, and issue a final verdict.

**Binding standard: `PRINCIPLES.md` at the repo root.** Read it first; every finding ties to a section.

## Inputs / scope

- *No argument* → target the **current diff** (`git diff` against `main`; if clean, the working-tree changes; if still empty, ask or sample `src/`).
- *Path or glob* (`/miri-audit src/mir/`), *branch range* (`/miri-audit feat..main`), *module* (`/miri-audit perceus`) → resolve and target that.
- *`--report-only`* → run the panel and produce the verdict, but **do not fix** (no Lead Miri Engineer dispatch).
- *`--with-minors`* → also fix minor findings, not just critical/major.

Resolve scope, print the file list back, and state the target before fanning out.

> **When to run the full panel.** This skill *is* the **Major**-tier validation path (PRINCIPLES.md §8). Run it for a Major-tier change, or whenever the user explicitly asks for a full audit/security-check. For a **Standard**-tier diff, prefer the single `miri-reviewer` agent (cheaper, no consolidation overhead) and escalate here only if a §8.1 trigger appears.

## Procedure

1. **Read `PRINCIPLES.md`** (§8 tiers, §9 ownership matrix, §10 severity rubric) and capture the diff/target so every specialist reviews the same thing. Run `make audit` once now — it owns the mechanical sweeps (unwrap/expect, stdlib-name leaks, `_ =>` over Miri enums, oversized functions, banners, comment rot). Its output is the mechanical-findings list; specialists do **not** re-grep these.
2. **Fan out the specialist panel in parallel** (single message, multiple `Agent` calls — read-only, independent). Each owns a disjoint axis set per §9; tell each to check **only its owned axes**, trust `make audit` for mechanical findings, and return a ranked list citing §10 severities:
   - `lead-software-architect` — layer boundaries, SOLID *judgment*, DRY, God-objects, altitude. (Not the mechanical sweeps — those are `make audit`'s.)
   - `lead-security-engineer` — Perceus UAF/double-free, FFI/ABI corruption, bounds/overflow, `unsafe` soundness, user-input panics, GPU overruns.
   - `lead-qa-engineer` — coverage gaps, green-washed/duplicate/misnamed tests, missing error paths and edge cases, adversarial repros.
   - `lead-rust-engineer` — ownership/borrow ergonomics, clone/alloc hygiene, iterator-vs-loop, perf, error-handling *shape*.
   - `lead-compiler-architect` — design soundness, IR/`Place`/terminator shape, monomorphization, residency/effect model, visitor-contract completeness, lowering-seam placement.
   - `lead-gpu-engineer` — **only if** the target touches WGSL / `src/runtime/gpu/` / residency / `gpu for|fn|let|var`. Skip otherwise and note N/A.
   Pass each the exact target.
3. **CTO consolidation (you do this yourself).** De-duplicate overlapping findings into one severity-ordered list, one owner each per §9. **Selective verification:** spot-check at the cited `file:line` only (a) every **critical**, and (b) any finding two specialists ranked at **different** severities. Trust uncontested majors that cite a line — do not re-read them. Drop findings you can't reproduce; upgrade anything under-rated. Adjudicate genuine cross-owner conflicts (§9) with evidence. Apply a practical-sense check: does the implementation make sense? (Optionally delegate this synthesis to the `lead-cto` agent, handing it the reports + diff.)
4. **Fix loop (skip if `--report-only`).** Dispatch the `lead-miri-engineer` with the consolidated critical + major findings (+ minor if `--with-minors`), by number, with file:line and the agreed fix. It applies fixes via TDD where behavior changes. **Critical and major are never left undone.**
5. **Re-validate (tight).** Run the gate (or spawn `miri-test-runner`): `make format` (empty diff) → `make lint` (clean) → `make build` → `make test` (`cargo test --test mod`, exact counts) → `make audit` (clean for touched files). **Re-run a specialist only if the fix changed code in its owned axis (§9) AND the fix was non-mechanical** — a mechanical fix (rename, extract, unwrap→`?`) is covered by `make audit` + tests, no re-panel.
6. **Loop** steps 3–5 until no open critical/major and the gate is green. If the same root cause survives three fix attempts, stop and surface it to the user — do not churn.
7. **Final CTO verdict** (format below), embedding each specialist's report.

## Final report format

```
# Miri Audit — CTO Verdict — <scope>
Status: DONE | NOT DONE (blockers open) | DONE-WITH-DEFERRED-MINORS
Gate: format <clean> | lint <clean> | build <clean> | test <N passing / M failing / K ignored> | audit <clean>

## Mechanical sweep (make audit)
<unwrap/stdlib-name/_=>/oversize/banner counts — clean or list>

## Specialist roll-up
Lead Software Architect: <grades A/B/C/F per dimension + headline>
Lead Security Engineer:  <headline finding + count>
Lead QA Engineer:        <coverage verdict + count>
Lead Rust Engineer:      <headline + count>
Lead Compiler Architect: <SOUND | SOUND-WITH-RISKS | UNSOUND>
Lead GPU Engineer:       <headline, or N/A>

## Consolidated findings (de-duped, re-ranked — §10 severities, §9 owners)
1. [critical] <finding> — owner — status: fixed/open
...

## Challenges & adjudications
- <conflict / missed axis> → <decision + evidence>

## Fixes applied
<diff summary + test-count delta>

## Deferred (minors only, with reason + follow-up recorded)
```

## Hard rules

- **Critical and major findings must be fixed** before DONE (unless `--report-only`, which still names them as blockers). Minor may be deferred only with explicit reason and a `notes/PLAN.md` follow-up.
- **One axis, one owner (§9).** Each specialist checks only its owned axes; `make audit` owns the mechanical sweeps. Do not have two agents re-raise the same axis — re-raised duplicates at differing severities are noise, not conflict.
- **Selective verification (§10).** Spot-check criticals and split-severity findings only; trust uncontested majors that cite a line. Don't re-read everything — that is the slow path you are avoiding. A finding you can't reproduce is dropped, not shipped.
- **Use the §10 severity rubric** — no agent invents its own critical/major/minor.
- **Never declare DONE with `make test` red** or `make audit` reporting new violations in touched files.
- **A green suite over a wrong design is NOT DONE** — honor the Compiler Architect's UNSOUND verdict.
- Only the `lead-miri-engineer` edits source. Specialists and you are read-only.
- **Never commit, stage, push, or otherwise touch git.** No `git add`, `git commit`, `git push`, `git stash`, branch creation, or rebases — not by you and not by any subagent. Leave all fixes in the working tree for the user to review and commit themselves. If a subagent is dispatched, instruct it explicitly not to run any git write commands.
- Always `cargo test --test mod` — never `--test integration`.
