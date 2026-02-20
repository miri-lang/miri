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
    - [Classes](#classes)
    - [Traits](#traits)
    - [Super Calls](#super-calls)
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
String               // string
bool                 // boolean
[int]                // list of ints
{String: float}      // map
(int, String)        // tuple
{int}                // set
Type?                // nullable type
:symbol              // symbol
```

### Type Aliases

```miri
type MyInt is int
type ID is String
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
async fn fetchData() String
    // ...

gpu fn compute(data [float]) [float]
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
let l2 List<String> = ["a", "b", "c"]
let first = l1[0]
let last = l1[-1]
let sublist = l2[1..3]
```

### Maps

```miri
let m1 = {"a": 1, "b": 2}
let m2 Map<int, String> = {1: "one", 2: "two"}
let val = m1["a"]
```

### Tuples

```miri
let t1 = (1, "hello")
let t2 Tuple<int, String> = (2, "world")
let num = t1[0]
```

### Sets

```miri
let s1 = {1, 2, 3}
let s2 Set<String> = {"a", "b", "c"}
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
enum Status: Ok, Error(String)
```

---

## OOP Features

### Classes

Miri supports object-oriented programming via `class` declarations with single inheritance and trait implementation.

```miri
class Animal
    var name String
    
    fn init(name String)
        self.name = name
    
    fn speak()
        println("...")

class Dog extends Animal
    fn speak()
        println(f"{self.name} says: Woof!")

class Cat extends Animal implements Serializable
    fn speak()
        println(f"{self.name} says: Meow!")

    fn serialize() String
        return f"Cat({self.name})"s
```

Generic classes:

```miri
class Box<T>
    var value T
    
    fn init(value T)
        self.value = value
```

### Traits

Traits define interfaces that classes can implement:

```miri
trait Drawable
    fn draw()

trait Resizable
    fn resize(width int, height int)

trait Shape extends Drawable, Resizable
    fn area() float
```

Implementing traits:

```miri
class Rectangle implements Shape
    var width float
    var height float
    
    fn draw()
        println(f"Rectangle: {self.width}x{self.height}")
    
    fn resize(w int, h int)
        self.width = w
        self.height = h
    
    fn area() float
        self.width * self.height
```

### Super Calls

Use `super` to call parent class methods:

```miri
class SpecialDog extends Dog
    fn speak()
        super.speak()
        println("And I'm special!")
```

---

## Imports

```miri
// Imports all public entities from the math module.
// NOTE: `system` is the reserved keyword for the standard library.
use system.math

// Imports all public entities from the io module
use system.io

// Imports only the print and println functions from the io module
use system.io.{print, println}

// Imports the io and net modules, renaming net to network
// Notice how it's possible to selectively import not just entities, 
// but also modules 
use system.{io, net as network}

// Imports all public entities from the local.users.user module
// `local` is the reserved keyword for the current project.
// NOTE: local modules must always be imported with the full path, starting 
// with `local.`, even if it's in the same folder.
use local.users.user

// Imports all public entities from the module1.module2 module of the package.
// This is an example of how to import a module from an external package.
use some_package.module1.module2
```

---

## Symbols

Symbols are lightweight strings starting with `:`.

```miri
let status = :active
```
