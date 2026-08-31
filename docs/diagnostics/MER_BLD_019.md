## Rule

`miri fmt` renders a file from the program it parsed. A comment written where
the parsed program does not carry it would therefore be absent from the
rendered text, and rewriting the file would delete it.

Rather than write a file that has quietly lost something the author wrote,
`miri fmt` compares the comments in the file against the comments in the text
it was about to write, and refuses when any is missing. The file is left
exactly as it was.

## Before

```sh
miri fmt notes.mi
```

```
error[MER_BLD_019]: Formatting Would Lose Content
  = formatting would drop the comment `// a note`
```

## After

Move the comment above the declaration it describes, so that the declaration
carries it, and format again:

```sh
miri fmt notes.mi
```

## Reference

[Build and Command Line](../reference/build.md)
