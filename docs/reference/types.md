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
- An ordering operator (`<`, `<=`, `>`, `>=`) applied to a type that defines no ordering. The numeric types and `bool` order by value; every other type orders by implementing `Comparable` from `system.ops`. Equality is a separate capability and stays available either way.

## What It Warns About

Nothing here stops a build. Each names something the program would mean exactly the same without — the residue an edit leaves when the last reader of a name goes away.

- A local nothing reads. Naming the binding `_name` says the value is deliberately not read, and turns the report off. A loop variable is not reported: `for index in 0..3` names the iteration whether or not the body reads it, and neither is a binding at the file's own top level, which everything importing the file can read.
- A parameter no body reads. Naming it `_name` is what to write when the signature is fixed by something outside the function — a trait it implements, a callback shape it is passed to — and this body has no use for the value. A declaration with no body has no unused parameters.
- A `private` declaration the file never uses. `private` says the name is reachable from nowhere else, so nothing can ever reach it. Declaring it `public` is the spelling that keeps it: an exported name is used by whoever imports the module.
- A statement written after a `return`, `break` or `continue` in the same block. Only the first is reported, because every statement after it is unreachable for the same reason.

## Key Concepts

- **Type inference**: Miri infers types from context; explicit type annotations use width-pinning constructors (e.g., `i32(5)`)
- **Generics**: Type parameters can be bounded by traits
- **Auto-copy types**: Small, all-primitive types are automatically copied
- **Type compatibility**: Wider types accept narrower values with truncation; narrower types reject wider values

## Per-Code Detail

Use `miri explain MER_TYP_<code>` for detailed guidance on each type-checker diagnostic code.
