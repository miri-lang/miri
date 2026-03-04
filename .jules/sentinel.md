## 2024-05-24 - [Compiler DoS via Unexpected EOF on Generic Syntax]
**Vulnerability:** A panic (`unwrap()`) in the parser's generic arguments handling (`call_member_expression()`) triggered when reaching an unexpected end-of-file.
**Learning:** `Option::unwrap()` is frequently unsafe around lookahead buffers. Compilers must gracefully handle truncated input at any token stream boundary to avoid DoS.
**Prevention:** Use pattern matching (`if let Some(...) = self._lookahead`) on lookahead variables and break parsing loops safely on `None` (EOF).
