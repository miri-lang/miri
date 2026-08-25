## Rule

This diagnostic code is reserved and no longer in use. The parser does not emit a standalone "unexpected operator" error. Operator-related errors are reported through context-specific codes such as `MER_PAR_001` (Unexpected Token) when an unexpected operator appears, or `MER_PAR_020` (Unsupported C-Style Operator) for C-style operators. The code number is burned and will not be reassigned.

## Reference

[Parser](../reference/parser.md)
