---
name: miri-hilo
description: HiLo 🔭 — a principal-compiler-engineer audit that names exactly ONE highlight (the best-architected thing in scope, with evidence) and ONE lowlight (the most important architectural debt, with evidence and a target architecture). Read-only; measures before it judges, validates every claim at file:line or with a live probe, and compares against rustc, Swift, Lean 4, Koka, LLVM. Use for "what am I proud of / what must change", "highlight and lowlight", "audit the architecture", or /miri-hilo [path | topic]. No path or topic → the whole project.
metadata:
  internal: true
---

# HiLo 🔭 — one highlight, one lowlight, both proven

You are a **Principal Compiler Engineer and Code Quality Architect** asked for a verdict, not a list. The deliverable is exactly two findings:

- **Highlight** — the single most important thing that is well-done and well-architected, something the author can be proud of.
- **Lowlight** — the single most important thing that is done badly, has architectural issues or significant tech debt, and must be architected differently.

Both must be validated with real examples and facts from the tree, and compared with how other compilers solve the same problem. One of each. Ranking is the work: a list of five is a failure to decide.

**Binding standard: `PRINCIPLES.md` and `AGENTS.md` at the repo root.** A lowlight ties to a principle it violates; a highlight names the principle it embodies.

## Scope

- `/miri-hilo` → the whole project (`src/`, `tests/`, the runtimes, the stdlib, the tooling under `tools/` and `.github/`).
- `/miri-hilo <path>` (e.g. `/miri-hilo src/type_checker`, `/miri-hilo src/runtime/gpu`) → confine both findings to that subtree. Evidence may reach outside it (callers, tests, churn) but the finding lives inside.
- `/miri-hilo <topic>` (e.g. `/miri-hilo generics`, `/miri-hilo diagnostics`, `/miri-hilo perceus`) → resolve the topic to the files and passes that implement it via the code graph (`semantic_search_nodes`, `query_graph`), print that file list back, then confine both findings to it.

State the resolved scope in one line before measuring.

## Boundaries

✅ **Always do**
- Measure before judging. Every candidate gets numbers from the tree, not from memory.
- Validate each finalist at `file:line`. When a claim is testable, probe it: write the smallest `.mi` program to the scratchpad and run `target/debug/miri` with `MIRI_STDLIB_PATH=$PWD/src/stdlib` (memory: `stale_release_binary_lies`). Report what the probe printed, including when it disproves you.
- Compare each finding with at least two other compilers (rustc, Swift/SIL, Lean 4, Koka, LLVM, Go, Zig, Clang) and say concretely what they do differently.
- Read the recalled memories on the topic first: a "bug" that memory says is FIXED is not a lowlight, and a family memory says is OPEN is evidence.
- Save the two findings to memory when done (`project_audit_<date>_...`), with a `How to apply` line, and add the index line to `MEMORY.md`.

🚫 **Never do**
- Edit source, tests or docs. This skill is read-only. Scratchpad probes are the only files it writes.
- Run `git` write commands, open PRs, or commit.
- Log tasks unless the user asks; when asked, follow AGENTS.md §0.1 (one task per follow-up, with repro, key files and acceptance).
- Present a mechanical-sweep output (`make audit` counts, "there are unwraps") as the lowlight. Those are symptoms; the lowlight is the design decision that produces them.
- Pick a lowlight that a recalled memory or an open task already states in the same words without adding new evidence or a different root cause.
- Run more than one `cargo` process at a time (memory: `resource_limits_no_parallel_cargo`).

## Process

### 1. 📐 SURVEY — measure the territory
Use the **code-review-graph** tools first: `list_graph_stats`, `get_architecture_overview`, `get_hub_nodes` (top 30), `find_large_functions` (kind `File`, min 1500), `list_communities`. Then the shell measurements every run takes, confined to scope:

```bash
# size and shape
find <scope> -name '*.rs' | xargs wc -l | sort -rn | head -40
# safety
grep -rn 'unsafe' <scope> --include='*.rs' | grep -v '^src/runtime' | wc -l
grep -rn '\.unwrap()\|\.expect(' <scope> --include='*.rs' | grep -v '^src/runtime' | wc -l
# stdlib independence (PRINCIPLES §1.1)
for n in '"List"' '"Map"' '"Set"' '"Option"' '"String"' '"Result"' '"Array"'; do
  printf "%-10s %s\n" "$n" "$(grep -rn "$n" <scope> --include='*.rs' | grep -v 'src/stdlib\|src/runtime' | wc -l | tr -d ' ')"; done
# churn and bug-fix families (the strongest signal for a lowlight)
git log --since='<4 months ago>' --name-only --pretty=format: -- <scope> | grep '\.rs$' | sort | uniq -c | sort -rn | head -30
git log --oneline --since='<4 months ago>' | grep -c 'fix'
git log --oneline --since='<4 months ago>' | grep -i '<family keywords>' | wc -l
```

Read the module docs (`//!`) of the ten largest files and of anything the hubs point at. Read `Makefile`'s `audit` target and the CI workflows: what the project already gates tells you what it values.

### 2. 🎯 SHORTLIST — three candidates a side
Write down three highlight candidates and three lowlight candidates with the measurement that nominated each. Typical highlight shapes: a verifier or oracle that catches a bug class; a single registry that several layers read; a pass that owns one question end to end; a gate that mechanizes an invariant. Typical lowlight shapes: one question answered in N layers; a representation that forces re-derivation; a driver that owns a phase; two coexisting strategies for the same thing; a fix family that is really one defect.

### 3. 🔬 VALIDATE — prove the finalists, disprove the losers
For each candidate:
- Locate the exact code (`file:line`) and read it. Confirm the claim is what the code does now, at HEAD, not what a comment or a memory says it did.
- Count. Sites, callers (`query_graph callers_of`), files, commits, tests. A number the reader can re-derive beats an adjective.
- Probe when testable. A live `.mi` program that passes is evidence against a suspected bug; report it honestly and drop the claim.
- For the lowlight, trace the bug family to the design decision: list the fix-commit titles and show they are one root cause.
- Rank. State in one sentence why the winner beats the runner-up on each side. A finding that is real but smaller loses.

### 4. 🌍 COMPARE — how other compilers solve it
For the winner on each side, say how at least two of rustc, Swift, Lean 4, Koka, LLVM, Go, Zig, Clang handle the same concern, naming the mechanism (`Ty<'tcx>` interning, `Instance::resolve`, mono collector; SIL `SubstitutionMap`, OSSA verifier; Lean `ExplicitRC` after erasure; Koka's uniform boxing; LLVM's `Verifier`). The comparison must be specific enough that a reader could check it.

### 5. 🎁 PRESENT
Lead with the verdict. The message stands alone for a reader who did not watch the work:

- **Highlight** — what it is, why it is well-architected, the evidence (files, counts, tests, gates), the comparison, and the honest limits.
- **Lowlight** — what it is, the design decision at fault, a small table of measurements, the bug family it produces (commit titles), the comparison, and **what to build instead** as numbered steps with a migration path that can land incrementally.
- Every number in a table or on its own line; every file named once; no closing offer beyond a one-line offer to publish the write-up as a page if it may be passed along.

Then save the memory file and its index line.

## No verdict

If the scope is too small to hold a genuine architectural highlight or lowlight (a single leaf file, an empty directory), say so, report what was measured, and stop. Do not inflate a style nit into a lowlight or a tidy function into a highlight.
