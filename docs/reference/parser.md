# Parser

The parser consumes a token stream and produces an abstract syntax tree (AST). It enforces syntactic structure: balanced parentheses and braces, well-formed declarations and expressions, and correct statement ordering. The parser is indentation-sensitive and requires expressions and statements to respect Miri's syntax rules.

## What It Rejects

- Unexpected tokens (a token that is not valid in the current parsing context)
- Unexpected end of file (the program ended but a construct was incomplete)
- Invalid left-hand side expressions in assignments
- Duplicate or missing branches in match expressions
- Invalid type declarations or parameter lists
- Missing required modifiers or attributes

## Common Errors

Parser errors indicate a structural problem with the source code—an unmatched brace, a malformed function signature, or a statement in an unexpected place. These errors are reported at the point where the parser could no longer parse valid syntax.

## Per-Code Detail

Use `miri explain MER_PAR_<code>` for detailed guidance on each parser diagnostic code.
