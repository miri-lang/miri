## Rule

`miri skill install` could not create the directory it writes into, or could not write the file itself. The path is built from `--target` (the working directory by default) and the directory the chosen agent reads: `.claude/skills/<name>/SKILL.md` for `claude`, `.agents/skills/<name>/SKILL.md` for `agents`, `cursor` and `codex`, and `skills/<name>/SKILL.md` for `generic`.

`miri build` also reports it when the executable would be written over the program's own source: without `-o` the executable is named after the source without its extension, so a source with no extension (`prog`) would be replaced by its own build. Name the output with `-o`.

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
