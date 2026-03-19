# Miri Language Specification (v0.1.0-alpha.3)

*This specification documents the currently implemented features of Miri v0.1.0-alpha.3.*

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
- [Classes](#classes)
- [Traits](#traits)
- [Closures](#closures)
- [Generics](#generics)

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

*Note: GPU codegen, closures with capture-by-reference (`out` closures), and cross-module visibility are planned for upcoming milestones.*

---

## Classes

Classes are reference types with named fields, constructors, methods, and single-inheritance.

### Declaration

```miri
class Animal
    protected name String

    fn init(n String)
        self.name = n

    fn speak()
        println(f"I am {self.name}")
```

### Constructor

The `init` method is the constructor. Fields are initialized inside `init` via `self.field = value`. Instantiation uses named arguments matching the `init` parameters.

```miri
let a = Animal(n: "Buddy")
```

### Inheritance

Use `extends` for single inheritance. Subclasses inherit all fields and methods from the parent.

```miri
class Dog extends Animal
    fn speak()
        super.speak()
        println("Woof!")
```

### `super` Calls

`super.method()` dispatches to the parent class implementation. `super.init()` chains to the parent constructor.

```miri
class Cat extends Animal
    fn init(n String)
        super.init(n)
        println("Cat created")
```

### Visibility Modifiers

| Modifier | Accessible from |
|----------|----------------|
| `public` | Everywhere (default for methods) |
| `protected` | Declaring class and all subclasses |
| `private` | Declaring class only |

```miri
class Counter
    private count int

    fn init()
        self.count = 0

    public fn increment()
        self.count = self.count + 1

    public fn value() int
        self.count
```

### Abstract Classes

Abstract classes cannot be instantiated. Abstract methods must be overridden in concrete subclasses.

```miri
abstract class Shape
    abstract fn area() float

class Circle extends Shape
    private radius float

    fn init(r float)
        self.radius = r

    fn area() float
        3.14159 * self.radius * self.radius
```

### Virtual Dispatch

When a variable is typed as a base class, method calls are dispatched at runtime via vtables to the correct subclass implementation.

```miri
let s Shape = Circle(r: 5.0)
println(f"{s.area()}")   // dispatches to Circle_area
```

---

## Traits

Traits define shared interfaces — a set of abstract (and optionally concrete) method signatures that classes can implement.

### Declaration

A trait contains method signatures. Methods without a body are abstract (required by implementors). Methods with a body are concrete (default implementations, overridable).

```miri
trait Greetable
    fn greet()

trait Printable
    fn to_string() String
        "object"   // default implementation
```

### Implementing Traits

Use `implements` to attach one or more traits to a class. The class must provide implementations for all abstract trait methods.

```miri
class Person implements Greetable
    fn greet()
        println("Hello!")
```

Multiple traits:

```miri
class SuperHero implements Runnable, Flyable
    fn run()
        println("running")
    fn fly()
        println("flying")
```

### Combining `extends` and `implements`

A class can extend a base class and implement traits simultaneously:

```miri
class Fish extends Animal implements Swimmer
    fn swim()
        println("swimming")
```

### Trait Inheritance

Traits can extend other traits using `extends`. Implementing a derived trait requires implementing all methods from the entire inheritance chain.

```miri
trait Shape
    fn area() float

trait ColoredShape extends Shape
    fn color() String

class RedCircle implements ColoredShape
    fn area() float
        3.14159 * 5.0 * 5.0
    fn color() String
        "red"
```

Multiple parent traits:

```miri
trait ReadWrite extends Readable, Writable
    fn readwrite()
```

### Default (Concrete) Methods

Traits can provide default method implementations. Classes inherit the default unless they override it.

```miri
trait Logger
    fn prefix() String
        "INFO"

    fn log(msg String)
        println(f"[{self.prefix()}] {msg}")

class AppLogger implements Logger
    fn prefix() String
        "APP"
```

### `Self` Type

Use `Self` in trait method signatures to refer to the implementing class's own type.

```miri
trait SameAs
    fn same(other Self) bool

class Point implements SameAs
    var x int
    var y int
    fn same(other Point) bool
        self.x == other.x
```

### Standard Library Traits

The `system.ops` module defines built-in traits used by the language:

| Trait | Used for |
|-------|----------|
| `Equatable` | `==` and `!=` operators |
| `Addable` | `+` operator |
| `Multiplicable` | `*` operator (repetition) |
| `Iterable` | `for x in collection` loops |

*Note: Trait objects (polymorphic variables typed as a trait, e.g. `let x Greetable = Person()`) require vtable support and are not yet implemented. Dynamic dispatch is available through class-typed variables.*

---

## Closures

Lambdas are first-class values. They can be stored in variables, passed as arguments, and returned from functions.

### Non-Capturing Lambda

```miri
let square = fn(x int) int: x * x
println(f"{square(5)}")   // 25
```

### Capturing Closure

A closure captures variables from the enclosing scope by value.

```miri
var base = 100
let add = fn(n int) int: base + n
println(f"{add(42)}")   // 142
```

Closures are represented as fat pointers `(fn_ptr, env_ptr)` at the ABI level. Captured variables are copied into an environment struct at the point of closure creation.

### Passing Closures

```miri
fn apply(f fn(int) int, x int) int
    f(x)

let double = fn(x int) int: x * 2
println(f"{apply(double, 7)}")   // 14
```

---

## Generics

Generic functions and types are monomorphized at compile time — a specialized copy is emitted for each unique set of type arguments.

### Generic Functions

```miri
fn identity<T>(x T) T
    x

fn first<T>(a T, b T) T
    a
```

Calling with different types produces separate compiled functions (`identity_int`, `identity_string`, etc.).

```miri
let n = identity(42)
let s = identity("hello")
```

### Generic Structs

```miri
struct Pair<T, U>
    first T
    second U

let p = Pair<int, String>(first: 1, second: "one")
println(f"{p.first}: {p.second}")
```

### Generic Classes

```miri
class Box<T>
    private value T

    fn init(v T)
        self.value = v

    fn get() T
        self.value

let b = Box<int>(v: 99)
println(f"{b.get()}")   // 99
```
