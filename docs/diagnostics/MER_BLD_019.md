## Rule

`miri fmt` renders a file from the program it parsed. Anything written where
the parsed program does not carry it would therefore be absent from the
rendered text, and rewriting the file would delete it.

Rather than write a file that has quietly lost something the author wrote,
`miri fmt` compares what the file holds against the text it was about to
write, and refuses when any of it is missing. The file is left exactly as it
was. Two things are compared: every comment, and every word — each keyword,
identifier and number. A word matters because losing one need not break
anything: `public` is the default visibility and `abstract` says what a
missing body already says, so a file that lost them still compiles and still
means the same thing, and still is not the file the author wrote.

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
