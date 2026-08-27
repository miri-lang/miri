# Grammar changelog

Records every change to `docs/grammar.peg`.

## How this file is enforced

- `docs/grammar.peg` carries exactly one `# version:` line, and this file must hold
  a matching `## Version <n>` entry.
- Each entry records a content hash of the grammar text. A test recomputes it, so
  **any** edit to the grammar fails the build until the author either bumps the
  version with a new entry or restates the hash here as a deliberate
  non-breaking change.
- The version increments only on an intentional breaking change.
- What the hash cannot decide is whether a change is *semantically* breaking for a
  downstream consumer. That is a human judgement; the hash only guarantees no
  grammar edit passes unnoticed.

## Version 1

**Content Hash**: `049892a031335427572d71d01edae0cb`

First publication. The grammar is token-level: it is written over the terminal
names the lexer produces rather than over source characters, because Miri's block
structure is carried by `INDENT`, `DEDENT` and `STMT_END` tokens that the lexer
synthesises from indentation, which no character-level PEG can express. The
lexical appendix in the grammar file documents each terminal's pattern and the
indentation algorithm, so a consumer can rebuild the token stream.

Validated by a two-sided differential gate against the recursive-descent parser:
both must accept every file in the accept corpus, and both must reject every
fixture in the reject corpus. The grammar covers 110 rules.

Known over-approximations are listed in the grammar file's preamble. The grammar
accepts token sequences the compiler rejects; it is sound for validation and for
grammar-constrained decoding, and is not a substitute for the compiler.

Breaking changes remain allowed while the language is still moving, and each one
is recorded here with a new version entry. Check the version before caching.
