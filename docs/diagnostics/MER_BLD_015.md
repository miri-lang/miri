## Rule

`miri skill install` could not create the directory it writes into, or could not write the file itself. The path is built from `--target` (the working directory by default) and the directory the chosen agent reads: `.claude/skills/<name>/SKILL.md` for `claude`, `.agents/skills/<name>/SKILL.md` for `agents`, `cursor` and `codex`, and `skills/<name>/SKILL.md` for `generic`.

## Before

```sh
miri skill install miri-lang --agent claude --target /read-only
```

## After

```sh
miri skill install miri-lang --agent claude --target ~/projects/app
```

## Reference

[Build and Command Line](../reference/build.md)
