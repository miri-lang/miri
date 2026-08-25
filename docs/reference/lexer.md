# Lexer

The lexer is the first stage of compilation, converting source text into a sequence of tokens. It recognizes keywords, identifiers, literals (integers, floats, strings, formatted strings, booleans, regexes), operators, and comment delimiters. It validates token syntax at the character level and enforces indentation-sensitivity.

## What It Rejects

- Invalid numeric literals (malformed binary, octal, or hex prefixes; non-digit characters in numeric sequences)
- Unclosed multiline comments (delimited by `/*` and `*/`)
- Unclosed string literals
- Unmatched quote pairs in formatted strings
- Invalid escape sequences or syntax within formatted string expressions

## Common Errors

Lexer errors are caught early and reported with precise line and column locations. Integer literals that are too large for their inferred type are not caught here; that check happens later in the type checker, which compares the literal against the inferred integer width.

## Per-Code Detail

Use `miri explain MER_LEX_<code>` for detailed guidance on each lexer diagnostic code.
