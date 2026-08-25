## Rule

No check emits this code. It was assigned when the type checker reported an assignment to a binding declared with `let`
through its own dedicated error variant. Those per-variant errors were later
regrouped into semantic families, and this condition is now reported as
`MER_TYP_042` (Immutability Violation), which carries the specific case in its message.

Nothing in the compiler constructs this diagnosis any more. The number stays
reserved so it is never handed to a different check: a tool that recorded it
against an older compiler can still resolve what it once meant, and will not be
misled by seeing it reused for something unrelated.

## Reference

[Type Checker](../reference/types.md)
