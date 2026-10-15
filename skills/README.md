# Miri Agent Skills

Publishable skills that teach code-generation agents to write Miri, following the **Agent Skills Standard** — one directory per skill with a `SKILL.md` file — so `npx skills add`, Claude Code, Cursor, and Codex consume them unmodified.

## Three Skills at Launch

### 1. **miri-lang** — Core Language
The short core for writing `.mi` source files: the syntax in one example, where Miri differs from Python and Rust, the traps that cost agents the most, how to look up the rest (`miri view`, `miri explain`), and the verification loop. It is kept to about 150 lines because an agent re-reads it on every later turn; API lists and long examples belong behind those lookups, not in the page.

**Activation triggers:** "writing Miri source," "Miri syntax," "`.mi` files," "editing Miri programs."

### 2. **miri-gpu** — GPU Programming (part 2)
GPU kernels, residency, `forall` and `gpu forall`, persistent device buffers, atomic operations, and `gpu frame`. Covers scalar-width constraints and when to use the GPU path.

### 3. **miri-testing** — Testing & The Miri Test Runner (part 2)
The `miri test` framework, test attributes, assertion semantics, and restrictions on top-level statements.

## Install from the Compiler

Put the compiler on `PATH` once, from a clone of this repository:

```bash
make install        # builds the release compiler and links it into ~/.local/bin
```

Then, in each project an agent works on:

```bash
miri skill install                  # Claude Code: .claude/skills/<name>/SKILL.md
miri skill install --agent agents   # Cursor, Codex, OpenCode, Windsurf, Gemini CLI: .agents/skills/
miri skill install --agent generic  # anything else: skills/<name>/SKILL.md
```

The installed compiler carries the skills that match it, and stamps each file
with its version and the SHA-256 of the body it wrote.

### Updating

After `git pull && make install`, run `miri skill install` again. A pack nobody
edited since it was installed is replaced; one that was edited is refused
(`MER_BLD_014`) and left alone, and `--force` replaces it anyway.

### Manual Copy

Skills live in this repository at `skills/<name>/SKILL.md`:

```bash
mkdir -p .claude/skills/miri-lang
curl -o .claude/skills/miri-lang/SKILL.md \
  https://raw.githubusercontent.com/miri-lang/miri/main/skills/miri-lang/SKILL.md
```

A copied file carries no version stamp, so `miri skill install` will not
replace it without `--force`.

### With `npx skills`

The [skills CLI](https://github.com/vercel-labs/skills) installs a pack straight
from this repository, with no Rust toolchain:

```bash
npx skills add miri-lang/miri --skill miri-lang
```

It writes `.agents/skills/miri-lang/SKILL.md`, links it into
`.claude/skills/` for Claude Code, and records the source in
`skills-lock.json`. The pack comes from `main`, not from the compiler you run,
and carries no version stamp, so it can describe a newer language than your
`miri` accepts and `miri skill install` treats it like a manual copy. The agent
still needs `miri` on `PATH` for its verification loop, which is why the
website's guide installs packs with `miri skill install`.

The contributor workflows under `.claude/skills/` set `metadata.internal: true`
so the CLI does not offer them; `cargo test --test mod skills` fails when one
does not.

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

Body: ≤400 lines (the gate); `miri-lang` aims for about 150. Its sections:

1. **Core Syntax** — one compact example, not a reference.
2. **Where Miri Differs** — syntax that does NOT exist, with fenced blocks marked `miri,fails=CODE`.
3. **Traps** and **Looking things up** — what goes wrong silently, and the commands that answer API questions.
4. **Verification Loop** — the one recommended loop: `miri check --format json` → edit (`miri fix --apply --yes` where a diagnostic carries a repair) → `miri test`. It is written once, in `miri-lang`; the other packs, the docs and the website defer to it.
```

## Quality Gate

The compiler gates every skill:

1. Each skill's `miri` code blocks compile or fail with the expected diagnostic code.
2. At least one block must demonstrate anti-hallucination (`fails=` directive).
3. Skill body stays under 400 lines (token economy for agent context).
4. Name in frontmatter matches the directory name.
5. Every `miri <command> --flag` a body names is a flag that command's `--help`
   accepts. The code blocks are compiled; the commands beside them were not
   checked at all until this, and a flag the binary does not have costs a reader
   an invocation and its trust in the rest of the page. Naming a flag inside a
   full command rather than on its own is what puts it under the gate.
6. The language pack's auto-applicable repair list is generated from
   `RepairId::all()` and matched verbatim, so a repair added, removed or
   reworded in the registry fails the gate until the pack says the same thing.
   The failure prints the block to paste in.

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
- **The website** (`../miri-lang.org`, a sibling checkout) — the "Miri for agents" guide renders these packs, vendored by `tools/gen_agent_docs.py`. The `agent_docs` tests fail when the vendored copy differs.

After editing a skill here, regenerate the website:

```bash
cd ../miri-lang.org
python3 tools/gen_agent_docs.py
```

## Future Skills

The roadmap includes:

- **miri-refinements** — refinement types, guards, and correctness predicates.
- **miri-diagnostics** — understanding error codes and using `miri explain` / `miri fix`.
- **miri-async** — async workflows and the `async gpu` surface.
