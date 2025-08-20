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
7. [Error Handling](#error-handling)
8. [Concurrency](#concurrency)
9. [Actors](#actors)
10. [Parallelism](#parallelism)
11. [GPU](#gpu)
12. [Imports](#imports)
13. [Symbols](#symbols)
14. [Guards](#guards)
15. [Syntactic Sugar](#syntactic-sugar)

---

## Core Concepts

- Indentation-sensitive (like Python, no `{}` or `end`).
- All control and definition headers end with a colon `:`.
- All top-level files define one public type, matching the file name (e.g., `Counter.mi` defines `Counter`).
- UpperCamelCase for types.
- Async, parallel, actor, and GPU programming are first-class.

---

## Types & Variables

### Declaration

```miri
x = 10               // inferred
var y = 20           // mutable
z int = 30           // explicitly typed
```

### Types

```miri
[int]                // array of ints
{string: float}      // dictionary
Map<string, int>     // generic type
```

---

## Functions

### Basic Syntax

```miri
square(x int) int:
  x * x
```

### Implicit Return

Only allowed for single-expression bodies.

### Explicit Return

```miri
sum(a int, b int) int:
  return a + b
```

### Lambdas

```miri
f = (x int) x * x
```

### Code Blocks (trailing blocks)

```miri
arr.map:
  (x) x * x
```

### Function Calls

Parentheses are optional:

```miri
print 'Hello'
log.warn 'Warning!'
```

---

## Control Flow

### Conditionals

```miri
if x > 0:
  print 'positive'
else:
  print 'non-positive'
```

### Loops

```miri
for i in 0..10:
  print i

while cond:
  ...

do:
  ...
while cond
```

---

## Collections

### Arrays

```miri
arr = [1, 2, 3]
arr[0]                // access
```

### Dictionaries

```miri
d = {'a': 1, 'b': 2}
d['a']                // access
```

### Iteration

```miri
for item in arr:
  print item

for k, v in d:
  print k, v
```

---

## Pattern Matching

```miri
match val:
  0:
    print 'zero'
  1 | 2 | 3:
    print 'low'
  x if x > 10:
    print 'large'
  default:
    print 'other'
```

---

## Error Handling

### Result Type

```miri
load(path string) Result<string, io::Error>:
  return fs.read(path)?
```

### `?` Operator

- Propagates `Err` immediately.
- Can only be used in `Result`-returning functions.

---

## Concurrency

### Async Functions

```miri
async fetch(url string) Result<string, net::Error>:
  return net.get(url)?
```

### Awaiting

```miri
html = await fetch('https://example.com')?
```

### Async Blocks

```miri
future = async:
  val = await work()
  return val * 2
```

---

## Actors

- All types can become actors.
- Spawned via `spawn Type.new`.

```miri
counter = spawn Counter.new()
counter <- inc()
value = await counter <- get()
```

---

arr.as_parallel.map(x: x * x)
books.as_parallel.each(book: book.read())

## Parallelism

### Parallel Loops

```miri
|| for item in collection:
  process(item)
```

### Parallel Map

```miri
result = || arr.map (x) x * x
```

### Parallel Method Broadcast

```miri
|| books.read
```

---

## GPU

### Defining GPU Functions

```miri
gpu add(a [float], b [float]) [float]:
  idx = thread_index()
  return a[idx] + b[idx]
```

### Invoking

```miri
result = add(vec1, vec2)
```

---

## Imports

```miri
use System.Math               // brings the system module Math
use Calc                      // local module, same folder
use MyProject.Path1.Path2.Lib // local module with a full path
use Utils as u                // alias
use add, sub from Ops         // selective
```

---

## Symbols

```miri
:name
status = :active
```

Used for enums, quick keys, etc.

---

## Guards (Function Input Validation)

```miri
transfer(amount float > 0.0) Result<void, string>:
  ...

register(user string in allowed_users):
  ...
```

Compiler injects checks to enforce.

---

## Syntactic Sugar

- `:` after all headers
- Function calls may omit `()`
- `guard` constraints inline in function signatures
- `.map`, `.filter`, `.reduce` are just methods
- Symbols via `:name`
- Code blocks via trailing `:` and indented lambda
- Parallel: `|| for`, `|| arr.map`, `|| collection.method`

---
