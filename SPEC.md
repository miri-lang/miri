# Miri Language Specification (v0.1.0-alpha.2)

*This specification documents the currently implemented features of Miri v0.1.0-alpha.2.*

---

## Table of Contents

- [Core Concepts](#core-concepts)
- [Types & Variables](#types--variables)
- [Functions](#functions)
- [Control Flow](#control-flow)
- [Pattern Matching](#pattern-matching)
- [Strings](#strings)
- [Structs](#structs)
- [Enums](#enums)
- [Tuples](#tuples)
- [Collections](#collections)
- [Option Types](#option-types)
- [Type Aliases](#type-aliases)
- [Imports](#imports)
- [Memory Model](#memory-model)

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

### Enum Destructuring

```miri
enum Wrapper
    Value(int)
    Empty

let w = Wrapper.Value(42)
match w
    Wrapper.Value(n): print(f"{n}")
    Wrapper.Empty: print("empty")
```

### Tuple Destructuring

```miri
let t = (10, 20)
let sum = match t
    (a, b): a + b
```

---

## Strings

```miri
let s = "Hello"
let name = "Miri"
let f = f"Hello, {name}"
```

---

## Structs

Structs are value types with named fields.

```miri
struct Point
    x int
    y int

let p = Point(x: 10, y: 20)
println(f"{p.x}")
```

Structs with mixed-type fields, including managed types:

```miri
struct User
    name String
    age int

let u = User(name: "Alice", age: 30)
```

Structs can be passed to and returned from functions:

```miri
fn offset(p Point, dx int, dy int) Point
    Point(x: p.x + dx, y: p.y + dy)
```

Small structs (all primitive fields, <= 128 bytes) are auto-copy — assignment produces a bitwise copy with no reference counting overhead.

---

## Enums

Enums support variants with and without associated data.

```miri
enum Color
    Red
    Green
    Blue

enum Shape
    Circle(float)
    Rect(float, float)
    None
```

Construction and matching:

```miri
let s = Shape.Circle(3.14)

match s
    Shape.Circle(r): println(f"radius: {r}")
    Shape.Rect(w, h): println(f"{w} x {h}")
    Shape.None: println("none")
```

---

## Tuples

Tuples are fixed-size, heterogeneous collections accessed by index.

```miri
let t = (1, "hello", true)
println(f"{t[0]}")   // 1
println(f"{t[1]}")   // hello
```

Tuples support destructuring in match expressions:

```miri
let pair = (10, 20)
let sum = match pair
    (a, b): a + b
```

---

## Collections

### Array

Fixed-size, stack-friendly collection. Type syntax: `[T; Size]`.

```miri
use system.collections.array

let nums = [1, 2, 3]
let first = nums.element_at(0)
println(f"{nums.length()}")
```

Methods: `length`, `element_at`, `set`, `is_empty`, `first`, `last`, `contains`, `index_of`, `reverse`, `sort`.

### List

Dynamic, growable collection. Type syntax: `[T]`.

```miri
use system.collections.list

var items = List([1, 2, 3])
items.push(4)
println(f"{items.length()}")
```

Methods: `length`, `element_at`, `get`, `set`, `push`, `pop`, `insert`, `remove`, `remove_at`, `clear`, `is_empty`, `first`, `last`, `contains`, `index_of`, `last_index`, `reverse`, `sort`.

### Map

Key-value collection. Type syntax: `{K: V}`.

```miri
use system.collections.map

let scores = {"Alice": 95, "Bob": 87}
println(f"{scores["Alice"]}")

var m = {"x": 1}
m["y"] = 2
```

Methods: `length`, `get`, `set`, `contains_key`, `remove`, `clear`, `is_empty`, `keys`, `values`.

The `get` method returns an option type (`V?`) — use pattern matching to handle missing keys safely.

### Set

Unordered collection of unique elements. Type syntax: `{T}`.

```miri
use system.collections.set

let s = {1, 2, 3}
if 2 in s
    println("found")
```

Methods: `length`, `element_at`, `add`, `contains`, `remove`, `clear`, `is_empty`.

The `in` operator checks set membership.

### Iteration

All collections support `for..in` iteration:

```miri
for item in items
    println(f"{item}")
```

---

## Option Types

Option types represent values that may or may not be present. Denoted with `?` suffix.

```miri
let x int? = None
let y int? = 42
```

### Unwrapping with `if let`

```miri
fn greet(name String?)
    if let Some(s) = name
        println(f"Hello, {s}")
```

### Matching

```miri
let val int? = 10
match val
    Some(n): println(f"got {n}")
    None: println("nothing")
```

### Coalesce operator

```miri
let x int? = None
let y = x ?? 42
```

The type checker rejects direct use of option types without an explicit check — preventing null pointer errors at compile time.

---

## Type Aliases

Type aliases create semantic names for existing types.

```miri
type ID is String
type Pair is (int, int)
type IntArray is [int; 3]
type ScoreMap is {String: int}
```

Aliases are transparent to the type checker and codegen — they are fully interchangeable with their underlying type.

---

## Imports

```miri
// Imports all public entities from the io module
use system.io

// Imports only the print and println functions from the io module
use system.io.{print, println}
```

### Standard Library Modules

| Module | Contents |
|--------|----------|
| `system.io` | `print`, `println`, `eprint`, `eprintln` |
| `system.string` | String class with intrinsics |
| `system.collections.array` | Array methods |
| `system.collections.list` | List methods |
| `system.collections.map` | Map methods |
| `system.collections.set` | Set methods |

---

## Memory Model

Miri uses a hybrid memory model with no annotations required from the programmer.

- **Auto-copy types**: Small, all-primitive structs and all primitive types are copied on assignment. No overhead.
- **Managed types**: Collections (`Array`, `List`, `Map`, `Set`), strings, and structs containing managed fields use reference counting.
- **Drop specialization**: When a managed type's reference count reaches zero, a type-specific drop function recursively releases all managed fields.

```miri
var a = [1, 2, 3]
var b = a            // RC incremented, both point to same data
a = [4, 5, 6]       // old array's RC decremented, freed if zero
```

*Note: Element-level RC (managed types inside collections) and full string ownership are deferred to a future release. See the project roadmap for details.*

*Note: OOP features (classes, traits, inheritance), closures, generic monomorphization, and GPU codegen are parsed but not yet supported in code generation. They will be covered in future specifications.*
