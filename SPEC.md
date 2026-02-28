# Miri Language Specification (v0.1.0-alpha.1)

*This specification documents the currently implemented features of Miri v0.1.0-alpha.1.*

---

## Table of Contents

- [Core Concepts](#core-concepts)
- [Types & Variables](#types--variables)
- [Functions](#functions)
- [Control Flow](#control-flow)
- [Pattern Matching](#pattern-matching)
- [Strings](#strings)
- [Imports](#imports)

---

## Core Concepts

- **Indentation-sensitive**: Like Python, no `{}` or `end` keywords are required for blocks.
- **Inline blocks**: Use a colon `:` for single-statement blocks.
- **Static Typing**: Explicit and inferred types.

---

## Types & Variables

### Declaration

```miri
let x = 10           // immutable, inferred
var y = 20           // mutable, inferred
let z int = 30       // explicitly typed
```

### Built-in Types

```miri
int                        // integer, size depends on the CPU
i8, i16, i32, i64, i128    // signed integers
u8, u16, u32, u64, u128    // unsigned integers
float                      // floating point, size depends on the CPU
f32, f64                   // floating point
bool                       // boolean
String                     // string
```

---

## Functions

### Basic Syntax

Parameters are defined as `name type` (no colon).

```miri
fn square(x int) int
    x * x
```

### Main Function

The entry point of a Miri program is the `main` function.

```miri
fn main()
    println("Program started")
```

If no main function is defined, the program wraps all top-level code in a main function.

---

## Control Flow

### If / Unless

```miri
if x > 10
    print("Large")
else if x > 5
    print("Medium")
else
    print("Small")

unless x == 0
    print("Non-zero")

// Ternary operator
let result = "Large" if x > 10 else "Small"

// Same, but with inline if
let result = if x > 10: "Large" else: "Small"
```

### Loops

```miri
// For loop
for x in 1..10
    print(x)

// While loop
while x > 0
    x -= 1

// Do-While loop
do
    x -= 1
while x > 0

// Until loop
until x == 0
    x -= 1

// Forever loop
forever
    print("Infinite")
```

### Inline Control Flow

```miri
if x > 10: print("Large")
for x in 1..10: print(x)
while x > 0: x -= 1
forever: print("Spinning")
```

---

## Pattern Matching

```miri
match x
    1: print("One")
    2 | 3: print("Two or Three")
    x if x > 10: print("Large")
    _: print("Other")
```

Inline match:

```miri
match x: 1: "One", 2: "Two"
```

---

## Strings

```miri
let s = "Hello"
let name = "Miri"
let f = f"Hello, {name}"
```

---

## Imports

```miri
// Imports all public entities from the io module
use system.io

// Imports only the print and println functions from the io module
use system.io.{print, println}
```

*Note: Further features (Collections, Structs, Enums, Classes, Generics, GPU functions) are parsed by the Miri compiler but are not yet supported for code generation in the Alpha 1 release. They will be finalized in future specifications.*
