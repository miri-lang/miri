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

Two keys are the same key when `==` says so: strings match by content, a class that defines or inherits `equals` matches through that method, and a value type matches by value. A class with no `equals` of its own or from a class it extends matches only the same instance.

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

### Namespace Collision Detection

Importing two modules that export the same name produces a compile error with suggestions for resolution (e.g., using aliased imports).

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

A class or struct may declare `fn drop(self)` (on a class, `fn drop()` means the same). Releasing the last reference to an instance is meant to run it exactly once. Known limitation: releasing a class instance through a trait-typed binding does not yet run the hook, nor release the instance's managed fields. A `drop` taking arguments, or a static one, is refused: the name belongs to the hook. A class finds its hook in the order every method follows: the nearest class in its chain declaring `drop`, else a trait default `drop`. So a subclass runs its base's hook, not a default `drop` a trait it implements supplies, and a subclass declaring its own `drop` runs only its own.

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
| `Comparable` | `<`, `<=`, `>` and `>=` operators |
| `Addable` | `+` operator |
| `Multiplicable` | `*` operator (repetition) |
| `Iterable` | `for x in collection` loops |

`Comparable` requires one method, `compare(other Self) int`, returning a negative number when `self` sorts first, zero when neither does, and a positive number when `self` sorts last. Each of the four ordering operators is derived from it by comparing that result against zero. The numeric types and `bool` order by value without it, and `String` implements it, so strings order by content. A named type that implements nothing is refused under an ordering operator (`MER_TYP_075`) rather than compared some other way. A generic parameter is accepted inside the body that declares it, the way arithmetic on a parameter is.

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
2. **The arguments it passes.** `identity(42)` binds `T` to `int`. A parameter declared as a trait or base class binds through the argument's own clauses: with `class Box implements Op<Foo>`, passing a `Box` where `Op<X>` is declared binds `X` to `Foo`.
3. **The type of the location the result goes into.** A parameter only the return type mentions is bound by the declared type of the binding, parameter or return type the result is stored in:

```miri
fn make<T>() Box<T>
    return Box<T>()

let b Box<String> = make()      // T = String
take(make())                    // T = the parameter type of `take`
```

A call whose type parameters are still unbound once its statement has been checked is refused (`MER_TYP_048`): no body can be compiled for it. That includes `let b = make()`, where nothing names the type, and a type parameter that appears nowhere in the signature (`fn noop<T>(x int) int`), which must always be written out: `noop<int>(5)`. Each bound a function declares on a type parameter (`T implements Named`) is checked against the type the call binds it to, however it was bound.

#### Distinct type parameters are distinct types

Inside a generic body, each type parameter the body declares is its own type. `fn cast<T, U>(a T) U` cannot `return a`, and `let x B = seed` is refused when `seed` is an `A` (`MER_TYP_002`): each parameter is bound by the caller, and nothing the body can see makes them the same. A parameter bounded by another is that other parameter — with `U extends T`, a `U` may be returned as a `T` — but two parameters sharing a bound are not: with `T extends Animal, U extends Animal`, a `U` is not a `T`, since a caller may bind `T` to a class `U` is not.

An arithmetic or comparison operator with a parameter operand — `a + 1`, `a * k`, `a < 10`, `a == 0`, `a + b` with `a T` and `b U` — is not decided by the body: each call answers for the pair of types it binds the operands to, and is refused there when the operator has no meaning at them (`inc("ab")` for `fn inc<T>(a T) T: return a + 1`). The pair is judged by the same rule as concrete operands, so `a * k` with `T = i8` and `k int` behaves as it does for an `i8` and an `int` written out. The body types the operation by its left operand, as for concrete operands: `a * k` is a `T`, and `k * a` is an `int` whatever `T` is bound to. A call whose operands give the operator another result — `s * v` with `s f32` and `v` bound to a vector — is refused. This holds however the call names the function: bare, through a module alias (`C.lt(x)`), or under an import alias (`use m.{lt as L}`).

A method of a generic class is checked at a type argument when the program uses it there: calls it, constructs or destroys an instance (`init`, `drop`), relies on it as an element's `compare` or `equals` — sorting a list of instances, applying an operator to one — or reaches it through a trait the instance is converted to and a call through that trait. The refusal is reported where the use is written: the construction, the sort, the operator, or the conversion. A method never used at an instantiation places no requirement on that argument and is not compiled for it, so `class Box<T>` with `fn lt() bool: return self.v < 10` may be built as a `Box<String>` as long as nothing calls `lt` on one.

The same holds through `super`: it names the parent at the arguments the `extends` clause gives it. With `class Child<X, Y, Z> extends Base<Z, X, Y>`, `super.init` takes a `Z`, an `X` and a `Y`, in that order, and `class Child extends Base<String>` reads `super.first` as a `String`.

#### Type arguments are invariant

A generic type's arguments must match exactly: a `Box<Dog>` is not a `Box<Animal>`, and a class implementing `Sink<Dog>` is not a `Sink<Animal>`, even though a `Dog` is an `Animal`. An instance's argument is both read and written through it, so accepting a subtype would let an `Animal` be stored where `Dog` readers expect a `Dog`.

The built-in collections follow the same rule for their element, key and value types: a `List<Dog>` is not a `List<Animal>`, a `List<i8>` is not a `List<i64>`, a `List<int>` is not a `List<int?>`, and a `Map<String, Dog>` is not a `Map<String, Animal>`. A literal is built at the type of the location it is written into rather than handed on, so it may be written where a wider element type is declared: `let ys Array<i8, 2> = [5, 6]` and `let m Map<String, int> = {}` are accepted. A collection constructor (`List([1, 2])`) is not a literal: it copies the literal it is given, so write the element type on it — `List<int?>([1, 2])`.

Function types are invariant in their parameters and their result: `fn(d Dog) String` is not a `fn(a Animal) String`, nor the other way round. A function value is called through the signature of the location holding it, so each argument is passed, and the result read back, at that signature's types.

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
