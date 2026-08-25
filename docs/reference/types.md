# Type Checker

The type checker validates that all types in the program are well-formed and consistent. It infers types for unannotated expressions, checks function calls against their signatures, validates generics, and enforces type compatibility in assignments and operations.

## What It Rejects

- Type mismatches (assigning a value of one type to a variable of another)
- Undefined variables, types, or functions
- Mismatched function arity or argument types
- Invalid generic arguments or type bounds
- Non-exhaustive match expressions
- Immutable variable assignments
- Field or method access on incompatible types
- Invalid type casts or conversions

## Key Concepts

- **Type inference**: Miri infers types from context; explicit type annotations use width-pinning constructors (e.g., `i32(5)`)
- **Generics**: Type parameters can be bounded by traits
- **Auto-copy types**: Small, all-primitive types are automatically copied
- **Type compatibility**: Wider types accept narrower values with truncation; narrower types reject wider values

## Per-Code Detail

Use `miri explain MER_TYP_<code>` for detailed guidance on each type-checker diagnostic code.
