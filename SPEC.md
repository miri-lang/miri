# Miri Language Specification (v0.1)

A modern, minimal, AI-friendly programming language for high-performance, concurrent, and GPU-accelerated applications.

---

## Table of Contents

- [Miri Language Specification (v0.1)](#miri-language-specification-v01)
  - [Table of Contents](#table-of-contents)
  - [Core Concepts](#core-concepts)
  - [Types \& Variables](#types--variables)
    - [Declaration](#declaration)
    - [Types](#types)
    - [Type Aliases](#type-aliases)
  - [Functions](#functions)
    - [Basic Syntax](#basic-syntax)
    - [Generics](#generics)
    - [Async \& GPU](#async--gpu)
    - [Lambdas](#lambdas)
    - [Guards](#guards)
  - [Control Flow](#control-flow)
    - [If / Unless](#if--unless)
    - [Loops](#loops)
    - [Inline Control Flow](#inline-control-flow)
  - [Collections](#collections)
    - [Lists](#lists)
    - [Maps](#maps)
    - [Tuples](#tuples)
    - [Sets](#sets)
  - [Strings \& Regex](#strings--regex)
    - [Strings](#strings)
    - [Regular Expressions](#regular-expressions)
  - [Pattern Matching](#pattern-matching)
  - [Structs](#structs)
  - [Enums](#enums)
  - [OOP Features](#oop-features)
  - [Imports](#imports)
  - [Symbols](#symbols)

---

## Core Concepts

- Indentation-sensitive (like Python, no `{}` or `end`).
- Inline blocks use a colon `:`, otherwise tabs or spaces.
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
int                  // integer, size depends on the CPU
i8, i16, i32, i64    // signed integers
u8, u16, u32, u64    // unsigned integers
float                // floating point, size depends on the CPU
f32, f64             // specific float sizes
string               // string
bool                 // boolean
[int]                // list of ints
{string: float}      // map
(int, string)        // tuple
{int}                // set
Type?                // nullable type
:symbol              // symbol
```

### Type Aliases

```miri
type MyInt is int
type ID is string
type Callback is fn(int) int
```

---

## Functions

### Basic Syntax

Parameters are defined as `name type` (no colon).

```miri
fn square(x int) int
    x * x
```

### Generics

```miri
fn identity<T>(x T) T
    x
```

### Async & GPU

```miri
async fn fetchData() string
    // ...

gpu async fn compute(data [float]) [float]
    // ...
```

### Lambdas

```miri
let f = fn(x int): x * x
let g = fn<T>(x T) T: x
```

### Guards

```miri
fn divide(a int, b int > 0) int
    a / b
```

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

## Collections

### Lists

```miri
let l1 = [1, 2, 3]
let l2 list<string> = ["a", "b", "c"]
let first = l1[0]
let last = l1[-1]
let sublist = l2[1..3]
```

### Maps

```miri
let m1 = {"a": 1, "b": 2}
let m2 map<int, string> = {1: "one", 2: "two"}
let val = m1["a"]
```

### Tuples

```miri
let t1 = (1, "hello")
let t2 tuple<int, string> = (2, "world")
let num = t1[0]
```

### Sets

```miri
let s1 = {1, 2, 3}
let s2 set<string> = {"a", "b", "c"}
```

---

## Strings & Regex

### Strings

```miri
let s = "Hello"
let f = f"Hello, {name}"
let single = 'Single quotes'
```

### Regular Expressions

```miri
let pattern = re"^\d+$"im
if text.matches(pattern)
    print("Match")
```

---

## Pattern Matching

```miri
match x
    1: print("One")
    2 | 3: print("Two or Three")
    x if x > 10: print("Large")
    (0, 0): print("Origin")
    re"^\d+$": print("Digit")
    _: print("Other")
```

Inline match:

```miri
match x: 1: "One", 2: "Two"
```

---

## Structs

Block style:

```miri
struct Point
    x int
    y int
```

Inline style:

```miri
struct Point: x int, y int
```

Generic structs:

```miri
struct Box<T>: value T
struct Node<T extends Equatable>: value T
```

---

## Enums

Block style:

```miri
enum Color
    Red
    Green
    Blue
```

Inline style:

```miri
enum Status: Ok, Error(string)
```

---

## OOP Features

Miri supports object-oriented programming patterns via `extends`, `implements`, and `includes`.

```miri
extends BaseClass
implements Interface1, Interface2
includes Mixin1, Mixin2
```

---

## Imports

```miri
use System.Math
use System.IO.*
use System.{IO, Net as Network}
```

---

## Symbols

Symbols are lightweight strings starting with `:`.

```miri
let status = :active
```
