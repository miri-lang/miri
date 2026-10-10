# Miri Language Specification (v0.2.0-alpha.4)

*This specification documents the currently implemented features of Miri v0.2.0-alpha.4.*

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
- [Imports & Modules](#imports--modules)
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

`int` and `float` are 64 bits on the host and 32 bits on a GPU (`i32` and `f32` there). Every rule below holds on every target, so a type holds every value of `int` or `float` only when it holds 64 bits' worth.

### Numbers of different types

One rule applies on every target, and it refuses a conversion only where a value could be lost:

- **Comparisons** between two integers, or two floats, compare their values: `small < n` with `small i8` and `n int` is never wrong, whichever side is written first.
- **Arithmetic and bitwise operators** between two integers, or two floats, compute at the narrowest type that holds every value of both on every target, so neither operand is truncated or wrapped and `a + b` is `b + a`: `i8 + int` is an `int`, `f32 + float` a `float`, and `u32 + i32` an `i64`. When no type holds both (`u128` and `i128`), the operator is refused; convert one operand with `as`. `min` and `max` combine their arguments the same way.
- **An integer with a float** is refused with an explicit `as`: neither holds every value of the other exactly.
- **A number written in the source** — a literal, or a `const` or `let` declared without a type and bound to one — has no width of its own and takes the type of the other operand or of the place it is stored: `small + 1` is an `i8`, `x * 2.0` with `x f32` an `f32`, `let k i32 = LIMIT` stores `LIMIT` as an `i32`. Each use takes its own width, so `let e = 0.04` is an `f32` beside an `f32` and stays a `float` where nothing narrows it. Arithmetic built only from such numbers takes a width as one number: `x / (2.0 * e)` computes `2.0 * e` at `f32`. A number does so only when its value fits, and for integer arithmetic, when every value it computes on the way fits: `m > -1` with `m u64` compares the two numbers. A `var` can be reassigned, so it takes `float` or `int` once, at declaration; write `var t f32 = 0.04` for another width.
- **Storing** a value — a binding, an assignment, an argument, a returned value, a collection element — requires the destination to hold every value of the source on every target. `let k i32 = n` with `n int` is refused, since an `int` holds more than an `i32` on the host; write `n as i32` to convert, which truncates or rounds as the cast says. An index is an `int`, or a narrower integer converted to one.

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

### Nested Patterns and Alternatives

A variant or a literal written inside another pattern is a further test the arm must pass, at any depth. String and float literals compare by value and a regex literal must match the string.

```miri
match o
    Some(Shape.Circle(s)): print(f"circle {s}")
    Some("a"): print("the string a")
    Some(_) | None: print("anything else")
```

The alternatives of an arm, `A | B`, are tried in order, and the arm is taken by the first one that matches. Every alternative must bind the same names, each at the same type, so the body reads the same names whichever one matched (MER_TYP_062). `_` binds nothing.

```miri
match p
    Pair.Left(n, s) | Pair.Right(s, n): print(f"{n} {s}")
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

A struct holds data and nothing else: it declares no methods, implements no traits and has no drop hook (MER_TYP_058). Behaviour belongs on a class, or in a function that takes the struct; a type that must run code when it is released is a class. What a struct supports is derived from its fields — `==` and `hash()` compare and hash them, and a struct whose every field is accelerable may be `gpu let`/`gpu var` with no marker.

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

Two arrays are equal under `==` when they hold equal elements in the same order; two lists likewise, and lists of different lengths are never equal. An array or list of elements `==` cannot compare cannot itself be compared.

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

Two keys are the same key when `==` says so: strings match by content, arrays and lists match element by element, a class that defines or inherits `equals` matches through that method, and a value type matches by value. A class with no `equals` of its own or from a class it extends matches only the same instance. A float key matches by the number it holds: `-0.0` and `0.0` are one key, and every NaN is one key — a NaN never equals itself under `==`, but a NaN stored in a map or set can be found and removed again. A type `==` cannot compare — a function value, or a type holding one — is refused as a key, and as a set element (MER_TYP_002). Keys are placed by their `hash()`, which agrees with `==`: two keys `==` calls equal hash alike. A type that writes its own `equals` must write a `hash` beside it (see `Hashable`), and one that does not is refused as a key, and as a set element (MER_TYP_079).

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

Elements are matched the way map keys are: two elements are the same element when `==` says so, so adding a string whose content the set already holds leaves the set unchanged.

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

### Type Sets

A type set names several types joined by `or`. It bounds a type parameter: each call binds the parameter to exactly one member.

```miri
type Real is f32 or float

fn sq<T is Real>(x T) T              // a named set as the bound
    return x * x

fn cube<T is f32 or float>(x T) T    // a set written in the bound itself
    return x * x * x

fn half(x Real) Real                 // the shorthand: fn half<T is Real>(x T) T
    return x * 0.5

fn main()
    let a f32 = 1.1
    println(f"{sq(a)} {sq(1.1)}")    // 1.21 1.2100000000000002 — the result keeps the argument's type
```

A set name written as a parameter or return type is the shorthand for a type parameter that set bounds, and every use of the same name in one signature is the same parameter: `fn clamp(x Real, lo Real, hi Real) Real` takes and returns one type, so `clamp(a, 0.0, 1.0)` with `a f32` is an `f32`, and two arguments at different members (`add(an_f32, a_float)`) are refused as they would be for `<T is Real>(x T, y T)`. Different set names are different parameters, appended after those the function declares, in the order they first appear; written type arguments bind them in that order (`half<f32>(0.5)`). The shorthand is for functions: a method or a lambda cannot take a set. The body cannot name the parameter the shorthand introduces — there `Real` is still the set — so a body that needs to write the type uses the explicit form.

A set may hold another set, which contributes its members: `type Number is Real or int`. A parameter bounded by a set satisfies a bound that holds every one of its members, so a `Real` value passes on to a `Number` parameter, while a `Number` one is refused by a `Real` bound.

A set is never the type of a value: a binding, field, collection element, parameter of a lambda or method, or any other place a value's type is written is refused (`MER_TYP_080`), because no single representation holds every member. A call binding a type outside the set is refused at the call, naming the set's members (`MER_TYP_037`).

A set is imported like any other declared name: with its module, or by name (`use local.shapes.{twice, Real}`).

`system.math` declares `Real` as `f16 or f32 or f64 or float`, and its functions take it: `sin(a)` with `a f32` is an `f32` on the host and on the device, as `f32::sin` in Rust and the WGSL overloads are. `abs`, `min` and `max` also take integers. A number literal takes the width of the argument beside it, so `mix(0.0, 1.0, t)` is an `f32` when `t` is.

---

## Imports & Modules

Miri supports multi-file projects with a module system that resolves imports, enforces visibility across module boundaries, and detects errors.

### Import Syntax

```miri
// Import all public entities from a module
use system.io

// Selective import
use system.io.{print, println}

// Module aliasing
use system.collections.list as L
use system.{io, collections.map as M}
```

### Local Modules

Files within a project are imported using the `local` prefix. The path maps directly to the file system relative to the project root.

```miri
// Imports from models/user.mi
use local.models.user

// Imports from utils/math.mi
use local.utils.math
```

### Standard Library Modules

Standard library modules are auto-discovered from the compiler's bundled stdlib path.

| Module | Contents |
|--------|----------|
| `system.io` | `print`, `println`, `eprint`, `eprintln` |
| `system.string` | String class with intrinsics |
| `system.collections.array` | Array methods |
| `system.collections.list` | List methods |
| `system.collections.map` | Map methods |
| `system.collections.set` | Set methods |

### Cross-Module Visibility

Visibility modifiers control which symbols are accessible from other modules:

| Modifier | Within module | From importing module |
|----------|--------------|----------------------|
| `public` | Yes | Yes |
| `private` | Yes | No |
| `protected` | Yes (class + subclasses) | No |

Top-level functions and classes are `public` by default. Fields and methods follow OOP visibility rules. Accessing a private symbol from another module produces a compile error.

```miri
// utils/helper.mi
public fn add(a int, b int) int
    a + b

private fn internal_detail() int
    42
```

```miri
// main.mi
use local.utils.helper

fn main()
    let x = add(1, 2)         // OK — add is public
    // internal_detail()       // Error — private to its module
```

A private type (class, struct, enum or trait) belongs to the module that declares it. Two modules may each declare a private type of the same name, and a program may declare a type of a name an imported module keeps private; each name refers to its own module's type. A private type's values never leave its module: a caller that would receive one — as a function's return or parameter, a field, a type argument, a closure type, `T?`, or a value bound by a pattern — is refused with "Type 'Helper' is not visible", just as if it had named the type itself.

### Namespace Collision Detection

Importing two modules that export the same name produces a compile error with suggestions for resolution (e.g., using aliased imports).

A module imported under an alias is a namespace: `M.f` names the `f` that module declares, whatever the program or another import calls `f`. Its names are also reachable unqualified, except one that something else — the program, or an earlier import — already declares; that name keeps its owner, and the alias is how the module's own is reached. Two modules declaring one name can therefore each be imported under an alias. A module's `private` declarations are never exported, so they collide with nothing.

### Circular Dependency Detection

If module `a.mi` imports `b.mi` and `b.mi` imports `a.mi`, the compiler reports the circular import chain with clear diagnostics.

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

### Reference Cycles

Reference counting never frees a cycle: an object that holds a reference to itself keeps its own count above zero and is never released. A store that puts an object back into one of its own fields is therefore refused (`MER_TYP_081`). The refused store writes into a place rooted at a binding that holds its object by reference — a class instance, a value held through a trait, or an `Array`, which has no copy on write — whether a field (`x.f`), a field of a field (`x.f.g`) or an element of either (`x.items[i]`). The value it refuses is that binding itself, a closure that captures it, or a variant (`Some(x)`), constructor call or literal holding one of those, including through either branch of a conditional.

```miri
class Ticker
    count int
    on_tick fn() int
    fn wire()
        self.on_tick = fn() int: self.count + 1   // MER_TYP_081: the closure captures self
```

To give an object a callback that works on it, make the object a parameter of the function type and pass it at the call: `on_tick fn(Ticker) int`, stored as `fn(t Ticker) int: t.count + 1` and called as `t.on_tick(t)`.

`List`, `Map` and `Set` copy on write, so a store into one a closure also holds copies the collection first and closes no cycle: `fns[0] = fn() int: fns.length()` is accepted.

The check sees the direct form only, where the stored value names the same binding as the target. A cycle closed through another name (`let u = t` then `t.on_tick = fn() int: u.count`) or through another object (`a.other = Some(b)` then `b.other = Some(a)`) is not refused, and leaks.

*Note: Element-level RC (managed types inside collections) and full string ownership are deferred to a future release. See the project roadmap for details.*

*Note: GPU codegen, closures with capture-by-reference (`out` closures), and full memory safety (Perceus+) are planned for upcoming milestones.*

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

### Field Initializers

A field may declare the value it starts at. A field without a type takes the type of its initializer; a field with both must have an initializer of that type. The initializer runs again for every instance, so each instance gets its own value, and it runs before `init`, so `init` can read or replace it. In a class without `init`, a constructor argument that sets the field replaces the initializer, which then does not run. A field without an initializer starts at its type's zero value. The initializer cannot read `self`.

```miri
class Crate
    var count = 7
    var label String = "crate"

let c = Crate()               // c.count == 7, c.label == "crate"
let d = Crate(label: "box")   // d.count == 7, d.label == "box"
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

### Traits a Base Class Implements

A trait a class implements is implemented by every class that extends it, whether or not the subclass names it again. A trait method the base declares answers for the subclass: a `Length` orders, compares equal, and sorts through the `compare` or `equals` a `Measure` declares, unless `Length` declares its own.

A subclass may name a trait its base does not implement. Where that trait's default collides with a method the base declares with a body, the base's method wins: `class Child extends Base implements Named` runs `Base.name()`, not `Named`'s default `name()`, whether it is called on a `Child` or through a `Named`. The same base method satisfies an abstract `name()` the trait requires. A subclass that wants the default's behavior declares the method itself.

A method that overrides one taking `Self` keeps the base's parameter type — it may be handed any `Measure` — so the override spells it `other Measure`. Its return may narrow to the subclass.

A trait method whose return names `Self` (`clone`, `concat`, `repeat`) is the exception. The body a subclass would inherit builds the base class, not the subclass, so every class extending such a declaration declares the method itself — with a body, or `abstract` to leave it to its own descendants. A subclass that does not is refused (`MER_TYP_057`).

```miri
class Measure implements Cloneable
    value int

    fn init(value int)
        self.value = value

    public fn clone() Self
        return Measure(self.value)

class Length extends Measure
    unit String

    fn init(value int, unit String)
        super.init(value)
        self.unit = unit

    public fn clone() Self
        return Length(self.value, self.unit)
```

### Trait Inheritance

Traits can extend other traits using `extends`. Implementing a derived trait requires implementing all methods from the entire inheritance chain, and a class implementing it implements every trait in that chain: a class implementing `trait Ranked extends Comparable` orders under `<`, `<=`, `>` and `>=` and sorts in a `List` exactly as one naming `Comparable` would, and the same holds for `Equatable`, `Addable`, `Multiplicable` and `Cloneable`.

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

Traits can provide default method implementations. A class inherits a default only for a method no class in its `extends` chain gives a body; the class wins over the trait. A class runs, for any method:

1. the nearest class in its `extends` chain — itself first — that declares the method with a body;
2. otherwise a default a trait supplies: the nearest class's `implements` list first, in the order it lists them; each listed trait is searched breadth-first through the traits it extends, in the order each lists its parents, so a trait is asked before its parents and an earlier parent before a later one; then the next class up the chain;
3. otherwise the method has no body: an abstract class may leave it so, and a concrete class is refused.

When two traits supply the same default, the first one reached in that order wins; the collision is not an error. A class that wants a different body declares the method itself.

An abstract declaration in the chain does not block a default: a default fills an abstract method no class gives a body. The nearest declaration being abstract does shadow every body further up the chain, so a class that re-declares an inherited method `abstract` makes its concrete descendants supply one — their own, an intermediate class's, or a trait default. A call through a trait-typed receiver reaches the same body a call on the class does.

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

### The `drop` Hook

A class or enum may declare `fn drop(self)` (on a class, `fn drop()` means the same); a struct holds data only and may not. Releasing the last reference to an instance is meant to run it exactly once. Releasing an instance through a trait-typed or base-class binding runs the hook of the instance's own class and releases its managed fields. A `drop` taking arguments, or a static one, is refused: the name belongs to the hook. A class finds its hook in the order every method follows: the nearest class in its chain declaring `drop`, else a trait default `drop`. So a subclass runs its base's hook, not a default `drop` a trait it implements supplies, and a subclass declaring its own `drop` runs only its own.

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
| `Hashable` | placing a value in a `Set` or as a `Map` key, consistently with `==` |
| `Comparable` | `<`, `<=`, `>` and `>=` operators |
| `Addable` | `+` operator |
| `Multiplicable` | `*` operator (repetition) |
| `Iterable` | `for x in collection` loops |

`Hashable` requires one method, `hash() int`, returning the same value for any two values `equals` calls equal. Every value answers `hash()`: a type whose equality is derived derives its hash from the same parts — a number its value (a float its number, so `-0.0` and `0.0` hash alike), a string its content, an optional its payload or a constant for `None`, a struct its fields, an enum its variant and that variant's payloads, a class without `equals` its identity — and `List` and `Array` implement it over their elements. A type that writes its own `equals` decides alone which of its values are equal, so only a `hash` of its own can agree with it: until it implements `Hashable`, it is refused as a set element or map key and wherever its `hash()` is called (MER_TYP_079), including through a generic body that hashes a parameter it is bound to. `hash_combine(seed, value)` folds the hashes of the parts `equals` compares into one: `return hash_combine(self.x.hash(), self.y.hash())`.

`Comparable` requires one method, `compare(other Self) int`, returning a negative number when `self` sorts first, zero when neither does, and a positive number when `self` sorts last. Each of the four ordering operators is derived from it by comparing that result against zero. The numeric types and `bool` order by value without it, and `String` implements it, so strings order by content. A named type that implements nothing is refused under an ordering operator (`MER_TYP_075`) rather than compared some other way. A generic parameter is accepted inside the body that declares it, the way arithmetic on a parameter is.

**Conformance without declaring.** A class meets the traits it names in its `implements` clause, and `String`, `List` and `Array` meet the ones their stdlib classes name. Every other type — a number, `bool`, a struct, an enum, a tuple, an optional — has no clause to write, and meets an operator trait wherever the language already gives it that trait's operator: `Equatable` where `==` compares it, `Comparable` where `<` orders it, `Addable` where `+` gives a value of its own type, `Multiplicable` where `*` with an `int` does, `Hashable` where it is `Equatable` and its hash is derived, and `Accelerable` where its values can live on a device. So `fn f<T implements Comparable>(x T)` takes an `f32`, and `fn same<T implements Equatable>(a T, b T)` takes a struct compared field by field. Such a type answers the trait's method through the operator, both inside a body bounded by the trait and when called directly: `a.equals(b)` is `a == b`, `a.concat(b)` is `a + b`, `a.repeat(n)` is `a * n`, and `a.compare(b)` is `-1`, `0` or `1` as `<` and `>` order the two (`0` for a NaN). Only these traits are met this way: a class meets a trait by declaring it, and a trait a program defines is never met by a method that happens to share a name. A trait method written with `Self` and called on a bounded parameter takes and returns that parameter: with `a T` and `T implements Addable`, `a.concat(b)` is a `T`.

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

### Nested Functions

A function declared inside another function's body is a closure bound to its name. It captures the enclosing locals it reads, can call any function in scope, and can call itself. It is visible from its declaration to the end of the enclosing block, and shadows a top-level function of the same name there.

```miri
fn main()
    let offset = 10
    fn shifted_sum(n int) int
        if n == 0
            return offset
        return n + shifted_sum(n - 1)
    println(f"{shifted_sum(3)}")   // 16
```

Captures are copies: assigning to a captured variable inside the body changes the closure's copy, not the enclosing variable.

A capture of a class instance is a copy of the reference, so a closure capturing an object and stored in one of that object's fields would hold the object alive forever. That store is refused; see [Reference Cycles](#reference-cycles).

### Methods Are Not Values

A method is called, never read as a value: `k.a` without a call is refused (`MER_TYP_078`), on a class instance, a trait-typed receiver and an enum alike. To hand a method on, wrap the call in a lambda, which names the parameters and shows that the receiver is captured:

```miri
class Scaler
    factor int
    fn apply(x int) int
        return x * self.factor

fn main()
    let s = Scaler(factor: 3)
    let triple = fn(x int) int: s.apply(x)   // not `s.apply`
    println(f"{triple(5)}")   // 15
```

A field whose type is a function is a value like any other field, and reading it does not call it.

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

#### Binding type parameters

A call binds each of the callee's type parameters from, in order:

1. **Type arguments written on the call.** `make<String>()` binds `T` to `String`. The call must write exactly as many type arguments as the function declares, or it is refused (`MER_TYP_036`). Inside a generic body, `make<T>()` binds the callee's parameter to the caller's own `T`.
2. **The arguments it passes.** `identity(42)` binds `T` to `int`. A parameter declared as a trait or base class binds through the argument's own clauses: with `class Box implements Op<Foo>`, passing a `Box` where `Op<X>` is declared binds `X` to `Foo`. A number literal binds a parameter only when no other argument does, so it takes the width of the argument beside it as it would beside an operator: `first(0.5, a)` with `a f32` binds `T` to `f32`, while `first(0.5, 1.5)` binds it to `float`.
3. **The type of the location the result goes into.** A parameter only the return type mentions is bound by the declared type of the binding, parameter or return type the result is stored in:

```miri
fn make<T>() Box<T>
    return Box<T>()

let b Box<String> = make()      // T = String
take(make())                    // T = the parameter type of `take`
```

A call whose type parameters are still unbound once its statement has been checked is refused (`MER_TYP_048`): no body can be compiled for it. That includes `let b = make()`, where nothing names the type, and a type parameter that appears nowhere in the signature (`fn noop<T>(x int) int`), which must always be written out: `noop<int>(5)`. Each bound a function declares on a type parameter (`T implements Named`, or `T is Real` for a [type set](#type-sets)) is checked against the type the call binds it to, however it was bound.

#### Distinct type parameters are distinct types

Inside a generic body, each type parameter the body declares is its own type. `fn cast<T, U>(a T) U` cannot `return a`, and `let x B = seed` is refused when `seed` is an `A` (`MER_TYP_002`): each parameter is bound by the caller, and nothing the body can see makes them the same. A parameter bounded by another is that other parameter — with `U extends T`, a `U` may be returned as a `T` — but two parameters sharing a bound are not: with `T extends Animal, U extends Animal`, a `U` is not a `T`, since a caller may bind `T` to a class `U` is not.

An arithmetic or comparison operator with a parameter operand — `a + 1`, `a * k`, `a < 10`, `a == 0`, `a + b` with `a T` and `b U` — is not decided by the body: each call answers for the pair of types it binds the operands to, and is refused there when the operator has no meaning at them (`inc("ab")` for `fn inc<T>(a T) T: return a + 1`). The pair is judged by the same rule as concrete operands, so `a * k` with `T = i8` and `k int` behaves as it does for an `i8` and an `int` written out. The body types the operation as the parameter operand's type, on whichever side it is written: `a * k` and `k * a` are both a `T`. A call binding `T` to a type the concrete operand would widen is refused there — `T = i8` with `k int` makes `k * a` an `int`, not an `i8` — while `T = i64` is accepted. A number literal beside a parameter takes the type the call binds the parameter to, as it would beside a concrete value. This holds however the call names the function: bare, through a module alias (`C.lt(x)`), or under an import alias (`use m.{lt as L}`).

A method of a generic class is checked at a type argument when the program uses it there: calls it, constructs or destroys an instance (`init`, `drop`), relies on it as an element's `compare` or `equals` — sorting a list of instances, applying an operator to one, adding one to a set or keying a map by one (`add`, a set or map literal, `in`, `m[k]`), holding a `Cloneable` one in any collection, which copies it through its `clone` whenever the collection is copied — or reaches it through a trait — a call through a trait the instance is converted to, or a call on a type parameter bounded by the trait (`fn go<X implements Lt>(x X): x.lt()`) that a call binds to the instance. The refusal is reported where the use is written: the construction, the sort, the operator, the set or map operation, the conversion, or the call binding the parameter. A trait default and a method an abstract class declares are compiled into each concrete class that runs them, so a call either makes on `self` runs that class's method at the instance's arguments. Inside one, a call on a parameter runs the method of the instance the caller passed; one that hands its own `self` on as a value — an argument, a binding, a returned value — uses every method called through that trait at the class it runs for. A conversion to a trait uses, at the converted instance, every method the program calls through that trait — wherever the call is written and whichever instance it runs on — not only those called on receivers the converted value reaches. This keeps whether a conversion is accepted independent of where its value flows: with `fn only_a(o Lt): return o.a()`, passing a `Box<String>` to `only_a` is refused when the program calls `b` through `Lt` anywhere, even on another class, and the refusal points at that call. A method never used at an instantiation places no requirement on that argument and is not compiled for it, so `class Box<T>` with `fn lt() bool: return self.v < 10` may be built as a `Box<String>` as long as nothing calls `lt` on one.

The same holds through `super`: it names the parent at the arguments the `extends` clause gives it. With `class Child<X, Y, Z> extends Base<Z, X, Y>`, `super.init` takes a `Z`, an `X` and a `Y`, in that order, and `class Child extends Base<String>` reads `super.first` as a `String`.

#### Type arguments are invariant

A generic type's arguments must match exactly: a `Box<Dog>` is not a `Box<Animal>`, and a class implementing `Sink<Dog>` is not a `Sink<Animal>`, even though a `Dog` is an `Animal`. An instance's argument is both read and written through it, so accepting a subtype would let an `Animal` be stored where `Dog` readers expect a `Dog`.

The built-in collections follow the same rule for their element, key and value types: a `List<Dog>` is not a `List<Animal>`, a `List<i8>` is not a `List<i64>`, a `List<i32>` is not a `List<int>` (`int` and `float` are types of their own, not stand-ins for every width), a `List<int>` is not a `List<int?>`, and a `Map<String, Dog>` is not a `Map<String, Animal>`. A literal is built at the type of the location it is written into rather than handed on, so it may be written where a wider element type is declared: `let ys Array<i8, 2> = [5, 6]` and `let m Map<String, int> = {}` are accepted. The same holds for a variant constructor whose payload is a literal: `E.R(5, s)` written where an `E<String, i128>` is declared is built at `i128`, while `E.R(n, s)` with `n` an `int` is an `E<String, int>`. A collection constructor (`List([1, 2])`) is not a literal: it copies the literal it is given, so write the element type on it — `List<int?>([1, 2])`.

Function types are invariant in their parameters and their result: `fn(d Dog) String` is not a `fn(a Animal) String`, nor the other way round. A function value is called through the signature of the location holding it, so each argument is passed, and the result read back, at that signature's types. Scalars agree only with the same scalar here too: a `fn(x i32) i32` is not a `fn(x int) int`, since calling it there would truncate every argument and result to 32 bits. Write a fold at its collection's element type — `[1, 2, 3].reduce(0, fn(a int, b int) int: a + b)` — which on the GPU is lowered to 32 bits all the same.

A class that extends or implements a generic type must write that type's arguments: `class B<X> extends A<X>`, never `class B<X> extends A` (`MER_TYP_036`). The supertype's members are typed by the position of its parameters, which a bare clause does not state.

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

A generic parameter may stand for an integer instead of a type: a **value parameter**, like `Size` in `Buf<T, Size>`. Its argument is a compile-time constant — built from integer literals, `const`s and the value parameters in scope — and each distinct value names its own instantiation. A value argument counts what each instance holds, so it must be **greater than zero**: `Buf<String, 0>` and `Buf<String, -1>` are refused (MER_TYP_077), and an argument computed from a value parameter (`Buf<T, Size - 1>`) that reaches zero or below at an instantiation is refused there (MER_MIR_017). A count that may be zero or negative is an ordinary constructor argument. The type `[T; 0]` of the empty array literal `[]` is not a written argument and stays valid.

```miri
class Buf<T, Size>
    v T
    fn init(v T)
        self.v = v

let b = Buf<String, 2>("ab")    // ok
let z = Buf<String, 0>("ab")    // MER_TYP_077
```
