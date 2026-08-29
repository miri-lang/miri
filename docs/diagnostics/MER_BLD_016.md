## Rule

A skill this build carries does not begin with a readable header, so the compiler cannot say what it is. The header is the block between the first two `---` lines. It must name the skill, and the name must match the directory the skill is installed under, so the two cannot disagree about what a reader is getting. It must also describe the skill in one line, which is what an agent matches against to decide when to reach for it.

The skills are compiled into the binary, so this reports a fault in the sources this build was made from rather than anything the caller did.

## Before

```
---
name: miri-language
---

# Miri Language Essentials
```

## After

```
---
name: miri-lang
description: Writing Miri source files — syntax, type system, and iteration workflow
---

# Miri Language Essentials
```

## Reference

[Build and Command Line](../reference/build.md)
