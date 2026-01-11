# Integration Test Restructuring Plan

### 1. `variables.rs` (Basic State & Scoping)
- **Features**:
    - variable declaration (`let`, `var`).
    - Type inference vs explicit typing.
    - Mutability checks (reassignment).
    - Scope visibility (block scope).
- **Tests**:
    - Declare `let` and use it.
    - Declare `var`, use it, modify it, use it again.
    - Explicit definitions `let x int = 1`.
    - Nesting: variables in inner blocks shadowing or accessing outer variables.

### 2. `control_flow.rs` (Branching & Loops)
- **Features**:
    - `if`, `else if`, `else`.
    - `unless`.
    - `for`, `while`, `do-while`, `until`, `forever`.
    - `break`, `continue`.
    - Inline control flow (`if cond: stmt`).
- **Tests**:
    - `if` expressions returning values.
    - Logic flow with `unless`.
    - `for` loop over ranges.
    - `while` loop logic.
    - `break`/`continue` inside loops.

### 3. `functions.rs` ( procedures & Lambdas)
- **Features**:
    - Named function declarations.
    - Parameter passing (by value/ref behavior check).
    - Return values.
    - Recursion.
    - Lambdas / Anonymous functions.
    - Function guards.
- **Tests**:
    - Call simple function.
    - Recursive factorial/fibonacci.
    - Lambda assignment and call.
    - Passing lambda as argument.
    - Function guards filtering inputs.

### 4. `primitive_types.rs` (Numbers, Bools)
- **Features**:
    - Integer types (`i8`..`u64`).
    - Floats (`f32`, `f64`).
    - Booleans.
    - Casting/conversion (if supported).
- **Tests**:
    - specific type arithmetic (overflow usage if applicable).
    - Boolean logic (`true`, `false`, `!`).
    - Mixed operations (float + int) if implicit conversion exists.

### 5. `strings.rs` (Text Processing)
- **Features**:
    - String literals.
    - Interpolation (`f"..."`).
    - Concatenation.
    - Regex literals (`re"..."`).
- **Tests**:
    - Basic string equality.
    - String interpolation with variables.
    - Regex matching (if `matches` method works in integration).

### 6. `collections.rs` (Composite Data)
- **Features**:
    - Lists (`[...]`), indexing, slicing.
    - Maps (`{k:v}`), access.
    - Tuples (`(...)`), destructuring access.
    - Sets (`{...}`).
- **Tests**:
    - Create list, read index, modify index (if mutable).
    - List slicing.
    - Map read/write.
    - Tuple creation and access `t[0]`.

### 7. `structs.rs` (Data Structures)
- **Features**:
    - Struct definition.
    - Field access.
    - Generic structs.
- **Tests**:
    - Define struct, create instance, read field.
    - Update field (if mutable).
    - Generic struct usage (e.g. `Box<T>`).

### 8. `enums.rs` (Sum Types)
- **Features**:
    - Enum definition (simple & data-carrying).
    - Pattern matching equality.
- **Tests**:
    - Define enum, assign to variable.
    - Match against enum variants.

### 9. `pattern_matching.rs` (Match Expressions)
- **Features**:
    - `match` statement.
    - Value matching, range matching, guards.
    - Destructuring.
- **Tests**:
    - Match integer literal.
    - Match with guards (`x if x > 10`).
    - Match tuple.
    - Match multiple options `1 | 2`.

### 10. `oop.rs` (Classes & Inheritance)
- **Features**:
    - `extends`, `implements` (if runtime supports).
- **Tests**:
    - Simple inheritance (if testable via field access or type check).
