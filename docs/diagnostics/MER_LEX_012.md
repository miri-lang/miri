## Rule

An f-string ends at the first unescaped quote of the kind that opened it, and
every interpolation must be closed by `}` before that point. A `{` still open
when the literal ends is this error.

The commonest way to reach it is a string literal written inside the braces with
the same quote character: that quote ends the f-string where it stands, leaving
the interpolation unclosed. The braces are balanced, so counting them finds
nothing — the inner quote is what to change. Write the nested literal with the
other quote character, or bind the value to a name above the f-string.

## Messages

- `Invalid Formatted String Expression`

## Help

- `The '{' that opened this interpolation is never closed by a '}'. A quote inside the braces closes the f-string at that quote, so write a nested string literal with the other quote character.`

## Before

```miri
let name = "world"
let msg = f"greeting = {name.replace("o", "0")}"
println(msg)
```

## After

```miri
let name = "world"
let msg = f"greeting = {name.replace('o', '0')}"
println(msg)
```

## Reference

[Lexer](../reference/lexer.md)
