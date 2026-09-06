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

## Version 2

**Content Hash**: `9d929602eadc40bc37a6f8e60072b0c2`

Corrects the rules for the two constructs whose colon read only a same-line
body, and three rules that described `match` inaccurately. A consumer holding
version 1 rejects programs the compiler accepts, so this is a breaking change for
anything caching the rules.

**A colon may open an indented block.** `body` now reads
`COLON (block / statement) / block`: the body may sit on the same line as the
colon or indented on the lines below. That is what `if`, `unless`, `while`,
`until`, `for`, `forall`, `forever`, `do`, the `else` clause and a `gpu frame`
block have always accepted through this rule — version 1 admitted only the
same-line form. `anonymous_function` gains the same alternative, and keeps its
own rule rather than folding into `body` because its inline form takes an
expression where `body` takes a statement.

The parser had the same gap in exactly those two places: a match arm and an
anonymous function each rejected `header:` followed by a block. Both now accept
it, so the rules and the parser agree.

**A function with a block body is block-valued.** `block_valued_expression`
gains `block_bodied_function`, because a binding whose right-hand side ends in a
block consumes no trailing `STMT_END` — the same reason `match` and `if` are
listed there. Only the block spellings appear in that rule; an inline
`fn(x int) int: x + 1` is an ordinary expression and stays one.

**The match rules described a shape the parser never read:**

- Alternative patterns in one branch are separated by `PIPE`, not `COMMA`.
- A branch may carry a guard, spelled `IF expression`. `UNLESS` is not accepted
  here, though it is accepted as a postfix statement guard.
- An inline match — `match e: p: v, p: v` — had no rule at all. `COMMA` separates
  whole branches in that form, which is where version 1's comma belonged.

The grammar covers 112 rules. Both differential gates run against the accept and
reject corpora, and the accept corpus gains a program exercising every
combination of header and body placement for both constructs.

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
