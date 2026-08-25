# Runtime Errors and Traps

Runtime errors are exceptions that occur during program execution, not during compilation. They represent operations that are mathematically undefined or operationally invalid at runtime. The runtime system detects these conditions and terminates the program with an error message.

## What It Catches

- Division by zero (numerator divided by zero)
- Remainder by zero (a value modulo zero)
- Integer overflow (result exceeds the width of the target type)
- Invalid operands (values outside the valid range for an operation, e.g., negative input to square root)

## Guarding Against Runtime Errors

Most runtime errors can be prevented with type-checker-enforced guards. For example, check that a divisor is non-zero before dividing, or use a wider type to avoid overflow.

## Per-Code Detail

Use `miri explain MER_RT_<code>` for detailed guidance on each runtime error diagnostic code.
