# Miri Language Specification (v0.1)

A modern, minimal, AI-friendly programming language for high-performance, concurrent, and GPU-accelerated applications.

---

## Table of Contents

1. [Core Concepts](#core-concepts)
2. [Types & Variables](#types--variables)
3. [Functions](#functions)
4. [Control Flow](#control-flow)
5. [Collections](#collections)
6. [Pattern Matching](#pattern-matching)
7. [Structs](#structs)
8. [Enums](#enums)
9. [Imports](#imports)
10. [Symbols](#symbols)

---

## Core Concepts

- Indentation-sensitive (like Python, no `{}` or `end`).
- Inline blocks use a colon `:`.
- UpperCamelCase for types.
- Async, parallel, actor, and GPU programming are first-class.

---

## Types & Variables

### Declaration

```miri
let x = 10           // immutable, inferred
var y = 20           // mutable, inferred
let z int = 30       // explicitly typed
```

### Types

```miri
int                  // integer
float                // floating point
string               // string
bool                 // boolean
[int]                // list of ints
{string: float}      // map
(int, string)        // tuple
{int}                // set
Type?                // nullable type
```

---

## Functions

### Basic Syntax

```miri
fn square(x int) int
    x * x
```

### Implicit Return

The last expression in a block is returned.

### Explicit Return

```miri
fn sum(a int, b int) int
    return a + b
```

### Lambdas

```miri
let f = (x int) -> x * x
```

### Guards

```miri
fn divide(a int, b int > 0) int
    a / b
```

---

## Control Flow

### If / Else

```miri
if x > 10
    print("Large")
else
    print("Small")
```

### Loops

```miri
for x in 1..10
    print(x)

while x > 0
    x = x - 1
```

### Inline Control Flow

```miri
if x > 10: print("Large")
for x in 1..10: print(x)
```

---

## Collections

### Lists

```miri
let list = [1, 2, 3]
let first = list[0]
```

### Maps

```miri
let map = {"a": 1, "b": 2}
let val = map["a"]
```

### Tuples

```miri
let tuple = (1, "hello")
let num = tuple[0]
```

### Sets

```miri
let set = {1, 2, 3}
```

---

## Pattern Matching

```miri
match x
    1 -> print("One")
    2 -> print("Two")
    _ -> print("Other")
```

---

## Structs

```miri
struct Point
    x int
    y int

let p = Point(x: 1, y: 2)
```

Inline struct definition:

```miri
struct Point: x int, y int
```

---

## Enums

```miri
enum Color
    Red
    Green
    Blue
```

---

## Imports

```miri
use std.io
```

---

## Symbols

Symbols are lightweight strings starting with `:`.

```miri
let status = :active
```
