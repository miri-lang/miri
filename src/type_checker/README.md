# Type Checker

The `type_checker` module enforces Miri's static typing rules, ensuring that expressions are valid and operations are performed on compatible types before code generation begins.

## Overview

The Type Checker traverses the Abstract Syntax Tree (AST), assigns types to every expression, and validates that all type constraints (e.g., function arguments, assignments, field accesses) are satisfied. It also performs type inference.

## Architecture

-   **Context Environment (`Context`)**: The central state object that tracks variables, functions, and types currently in scope. It handles lexical scoping by pushing and popping environments as the traversal enters and leaves blocks.
-   **Validation Passes**: The checker validates declarations top-level constructs, statements, and deeply inspects expressions.
-   **Type Inference**: For variable declarations without explicit types (`let x = 42`), the type checker infers the type from the right-hand-side expression.
-   **Generics (`generics.rs`)**: Handles substitution and validation of type parameters for generic functions and collections. A parameter written as a trait or base class binds from the arguments the argument's own `implements`/`extends` clauses pass it.
-   **Call instantiation (`call_instantiation.rs`)**: Decides which instantiation of a generic function a call reaches — from the type arguments it writes out, the values it passes, and, for a parameter only the return type mentions, the type of the location its result is stored in. A call nothing binds is refused at the end of its statement; lowering never links a generic call to the shared body.
-   **Binding refinement (`binding_refinement.rs`)**: A local binding written without a type takes its initializer's type, which a variant constructor leaves open in the arguments its payload does not name. When a later store into the binding (an assignment, an index store, a call such as `push` on it) binds one of those arguments, the store records the binding's refined type and the function body is checked again, with the declaration taking the refined type as if it were written. A binding at module scope is still refused.
-   **Instantiation requirements (`instantiation_requirements.rs`, `used_methods.rs`, `compiled_instances.rs`)**: A generic body states what it requires of its parameters, and each place the program uses the body at concrete arguments answers those requirements by replaying the body's own checks there — a call, a construction, an element sort or set lookup, a conversion to a trait. After lowering, the pipeline answers every instance it compiled once more through `compiled_instance_refusals`, so a body lowering reaches in a way the checker did not foresee is refused rather than compiled unchecked, or, when only a vtable slot or an element thunk names it, withheld.
-   **Visibility Verification**: Enforces access control (`public`, `private`, `protected`) for class fields and methods across module boundaries.
-   **Hygiene (`hygiene/`)**: Reports what a file declares and never uses — an unread local, a parameter no body reads, an unused import, an uncalled private declaration — and statements written after a `return`, `break` or `continue`. Every report is a warning, because each names something the program would mean exactly the same without. It runs over the file being compiled and nothing else: an imported module is somebody else's file, and its private helpers are called from inside it.

## Design Principles

1.  **Multiple Errors**: Like the parser, the type checker is designed to report as many type errors as possible in a single run, utilizing a unified diagnostic system rather than failing immediately upon the first error.
2.  **Non-Destructive AST**: The type checker does not heavily mutate the AST. Type information necessary for later stages (MIR lowering) is either stored in a sidecar data structure (the Context) or annotated minimally.
3.  **Strictness**: It strictly enforces type compatibility, trait constraints, and OOP inheritance rules, guaranteeing that well-typed Miri programs will not encounter runtime type exceptions.
