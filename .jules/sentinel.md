## 2024-05-24 - [Compiler DoS via Unexpected EOF on Generic Syntax]
**Vulnerability:** A panic (`unwrap()`) in the parser's generic arguments handling (`call_member_expression()`) triggered when reaching an unexpected end-of-file.
**Learning:** `Option::unwrap()` is frequently unsafe around lookahead buffers. Compilers must gracefully handle truncated input at any token stream boundary to avoid DoS.
**Prevention:** Use pattern matching (`if let Some(...) = self._lookahead`) on lookahead variables and break parsing loops safely on `None` (EOF).

## 2024-05-25 - [Compiler DoS via Unchecked Indent Stack Unwrap]
**Vulnerability:** The lexer panics (`unwrap()`) when attempting to handle unclosed indentation structures because it assumes the `indent_stack` will never be empty while dedenting.
**Learning:** Compilers must not trust their own internal state tracking blindly when processing untrusted input streams. An unexpected EOF or malformed whitespace can deplete stacks faster than expected.
**Prevention:** Use safe pattern matching (e.g., `while let Some(...) = stack.last()`) and break cleanly when stacks are depleted during whitespace processing.
