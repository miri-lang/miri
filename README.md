# Miri: General Purpose Object-Oriented Language
## Goal
The goal is to combine good practices of object-oriented design and enforce them on programming language level. The final product should satisfy the definition of a modern general purpose programming language.

## Motivation
I believe the large amount of books and articles on “good software design” is a result of the excessive flexibility of modern programming languages. What is the point of giving a knife to a child and then attempting to teach them how to use the knife, after many cuts?
The human mind has flaws which result in poor software quality. Experience and knowledge increase software quality, but there’s no easy way to ensure constant delivery of high-quality code. Developers get bored, lazy, lose motivation. Product managers don’t really understand why we need “refactoring”.
The thing is: modern programming languages allow us to cut corners and create bad software. They allow us to act badly. It is that kind of democracy where killing or robbing people is lawful, but it’s frowned upon.
We all agree such terrible acts should be forbidden by law. Why do we have programming languages which OK with us being bad and then use all kinds of poLINTsmen to control our actions?
Let us create proper democracy, where bad practices are forbidden. `Miri` will be your linter, cop, guide, and book on good software design. If you use this language, you’ll have no chance to get bad.

## Key Principles
* Opinionated
* Object-oriented
* General purpose
* Modern

## Features
* Static typing.
* Folder structure and file names play key role in namespacing:
  * Folder name is part of a namespace.
  * File name is also a type name. Full type name example: `Blog/Users/User` corresponds to `Blog/Users/User.mi` file.
  * One file describes one type only.
* No variables. Data structures are immutable.
* Instance fields are prefixed with underscode: `_field`.
* Instance variables can’t change.
* Generic types.
* No null/nil reference.
* Types have destructors.
* No garbage collection, immediate cleanup.

## Example

Don’t be scared :) Everything has a reason.

### Folder Structure
```
HelloWorldApp/
  Program/
    _.mi
    run.mi
```

### Essential Code
#### HelloWorldApp/Program/_.mi

```
uses Global/System
  
is ConsoleProgram
```

#### HelloWorldApp/Program/run.mi
```
_console.writeLine 'Hello World!'
```

If you wonder why that tiny peace is split in two files, see how Miri programs are actually supposed to look like:

### With All Features Included
  
#### HelloWorldApp/Program/_.mi

```
it Provides an example of console application in Miri.

// Declaration of modules which are used by this type.
// Tabulation identifies nested modules.
uses Global/System
  IO
  IO/Fakes
  Collections
  
// Declaration of base types in format: is Type1, Type2, etc.
is ConsoleProgram

// Constructors.
// Instance variables are automatically inferred from parameters.
(:forTest, console Console)
  new(console, Array<String>.new)
```

#### HelloWorldApp/Program/run.mi
```
it Runs the program.

test Outputs "Hello World!" to console
  console = FakeConsole.new
  new(:forTest, console).run
check Buffer must have positive length
  console.hasOutput
check Buffer contains Hello World
  console.containsInBuffer 'Hello World!'

_console.writeLine 'Hello World!'
```

Unit tests are actually part of each function file. In other languages you would have 2 files, located in two different places, but both implementing or testing same stuff. In Miri TDD is actually part of the language. More than that, the
tests are then used for documentation of your code as examples.
So in one file you write documentation, tests and implementation.

## The Name
Miri is named after my daughter. It’s her nickname in our family. Her short name is Mira and her full name is Myroslava.
When she was about 6 month I saw StarTrek for the first time in my life. There was a girl Miri. We already called our daughter like that, so this is just funny coincidence.
Using girl’s name for programming language doesn’t bother me at all. Things should have value beyond their names. Same applies to people.
My daughter is very opinionated. She has her own way of doing things. This quality is included to Miri language :)
