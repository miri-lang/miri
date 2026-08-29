# Miri Agent Skills

Publishable skills that teach code-generation agents to write Miri, following the **Agent Skills Standard** — one directory per skill with a `SKILL.md` file — so `npx skills add`, Claude Code, Cursor, and Codex consume them unmodified.

## Three Skills at Launch

### 1. **miri-lang** — Core Language
The foundation for writing `.mi` source files. Covers Miri's indentation-sensitive syntax, the core type system (no type annotations on bindings), pattern matching, structs, enums, and classes. Includes an anti-hallucination section documenting syntax that does **not** exist and the verification loop for arriving at correct code.

**Activation triggers:** "writing Miri source," "Miri syntax," "`.mi` files," "editing Miri programs."

### 2. **miri-gpu** — GPU Programming (part 2)
GPU kernels, residency, `forall` and `gpu forall`, persistent device buffers, atomic operations, and `gpu frame`. Covers scalar-width constraints and when to use the GPU path.

### 3. **miri-testing** — Testing & The Miri Test Runner (part 2)
The `miri test` framework, test attributes, assertion semantics, and restrictions on top-level statements.

## Install from the Compiler

### In Claude Code

The installed compiler carries the skills that match it:

```bash
# Inside a Claude Code project
cd /path/to/project
miri skill install --agent claude
```

This writes skills to `.claude/skills/<name>/SKILL.md` so they're immediately available in your session.

### For Other Agents

- **Cursor:** `miri skill install --agent cursor`
- **Codex/Generic:** `miri skill install --agent generic` (writes to plain `skills/<name>/SKILL.md`)

### Manual Install via `npx skills add`

```bash
npx skills add <github-org>/miri --skill miri-lang
```

This requires the `skills` package manager. The same `SKILL.md` from the compiler is used in all installs.

### Manual Copy

Skills live in this repository at `skills/<name>/SKILL.md`. Copy them directly:

```bash
cp skills/miri-lang/SKILL.md ~/.claude/skills/miri-lang/SKILL.md
```

## Using Skills in Your Agent

Once installed, skills are available in your agent's system prompt or as separate context. Agents activate them by name:

- **Claude Code** activates by matching description tags: "writing Miri source" triggers **miri-lang**.
- **Cursor** and other editors consume the same `SKILL.md` format.

## Structure

Each skill is a single `SKILL.md` file with YAML frontmatter:

```markdown
---
name: <skill-name>
description: <one-line activation trigger>
---

# Title

Body: ≤400 lines. Three sections:

1. **Positive Grammar** — what the language supports (compact, no exhaustive reference).
2. **Anti-Hallucination** — syntax that does NOT exist (fenced code blocks, each marked `miri,fails=CODE`).
3. **Verification Loop** — workflow: `miri check` → `miri explain` → `miri fix --plan`.
```

## Quality Gate

The compiler gates every skill:

1. Each skill's `miri` code blocks compile or fail with the expected diagnostic code.
2. At least one block must demonstrate anti-hallucination (`fails=` directive).
3. Skill body stays under 400 lines (token economy for agent context).
4. Name in frontmatter matches the directory name.

Run locally:

```bash
cargo test --test mod skills
```

Run in CI:

```bash
make skills-check
```

## Miri vs the Website

- **This repository** (`skills/`) — the source of truth, embedded in the compiler binary.
- **The website** (`../miri-lang.org`, a sibling checkout) — publicly rendered copy via `gen_agent_skills.py`.

After editing a skill here, regenerate the website:

```bash
cd ../miri-lang.org
python3 tools/gen_agent_skills.py
```

## Future Skills

The roadmap includes:

- **miri-refinements** — refinement types, guards, and correctness predicates.
- **miri-diagnostics** — understanding error codes and using `miri explain` / `miri fix`.
- **miri-async** — async workflows and the `async gpu` surface.
