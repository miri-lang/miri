## Rule

The file `miri skill install` would write already exists and does not match what this build carries, so something has edited it since it was installed. The command leaves it alone rather than discarding the edit; `--force` replaces it.

## Before

```sh
# .claude/skills/miri-lang/SKILL.md has been edited since it was installed
miri skill install miri-lang --agent claude
```

## After

```sh
miri skill install miri-lang --agent claude --force
```

## Reference

[Build and Command Line](../reference/build.md)
