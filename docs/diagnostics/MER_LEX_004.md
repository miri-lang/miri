## Rule

This diagnostic code is reserved and no longer in use. Unclosed string literals are not reported as a distinct error by the lexer; an unterminated string is caught by the token regex engine and reported as `MER_LEX_001` (Invalid Token) instead. The code number is burned and will not be reassigned.

## Reference

[Lexer](../reference/lexer.md)
