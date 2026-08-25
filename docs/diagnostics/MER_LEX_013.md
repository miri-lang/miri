## Rule

This diagnostic code is reserved and no longer in use. Integer literal overflow is not detected by the lexer; the lexer tokenizes all well-formed integers regardless of magnitude. Overflow checking occurs later during type inference in the type checker, and is reported as `MER_TYP_068` (Integer Literal Does Not Fit In Type) instead. The code number is burned and will not be reassigned.

## Reference

[Lexer](../reference/lexer.md)
