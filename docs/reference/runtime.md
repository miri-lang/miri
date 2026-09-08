# Runtime Errors and Traps

Runtime errors are exceptions that occur during program execution, not during compilation. They represent operations that are mathematically undefined or operationally invalid at runtime. The runtime system detects these conditions and terminates the program with an error message.

## What It Catches

- Division by zero (numerator divided by zero)
- Remainder by zero (a value modulo zero)
- Integer overflow (result exceeds the width of the target type)
- Invalid operands (values outside the valid range for an operation, e.g., negative input to square root)
- An index outside the collection it was applied to, including a command-line argument asked for by a position that was not supplied
- An explicit `panic`, which the program raises when it has reached a state it cannot continue from

## Guarding Against Runtime Errors

Most runtime errors can be prevented with type-checker-enforced guards. For example, check that a divisor is non-zero before dividing, or use a wider type to avoid overflow.

## Per-Code Detail

Use `miri explain MER_RT_<code>` for detailed guidance on each runtime error diagnostic code.

## How a Trap Is Reported

A trap ends the program and reports itself twice over. The sentence a person reads goes to the program's own stderr and says which index or which divisor; a tool reads `miri run --format json`, where the trap arrives as an `MER_RT_*` diagnostic and `ok` is false. Every trap the runtime raises carries a code, so a run that ended in one is never reported as a run that succeeded.
