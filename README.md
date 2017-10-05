# Miri: General Purpose Object-Oriented Language
## Goal
The goal is to combine good practices of object-oriented design and enforce them on programming language level. The final product should satisfy the definition of a modern general purpose programming language.

## Motivation
I believe the large amount of books and articles on “good software design” is a result of the excessive flexibility of modern programming languages. What is the point of giving a knife to a child and then attempting to teach them how to use the knife, after many cuts?
The human mind has flaws which result in poor software quality. Experience and knowledge increase software quality, but there’s no easy way to ensure constant delivery of high-quality code. Developers get bored, lazy, lose motivation. Product managers don’t really understand why we need “refactoring”.
The thing is: modern programming languages allow us to cut corners and create bad software. They allow us to act badly. It is that kind of democracy where killing or robbing people is lawful, but it’s frowned upon.
We all agree such terrible acts should be forbidden by law. Why do we have programming languages which OK with us being bad and then use all kinds of poLINTsmen to control our actions?
Let us create proper democracy, where bad practices are forbidden. `Miri` will be your linter, cop, guide, and book on good software design. If you use this language, you’ll have no chance to get bad.

## Rules
* Classes always extend types or other classes.
* Instance variables are prefixed with underscode ``_variable``
* Instance variables can’t change.
* Every function produces new instance.
* No nil/null references.
* No conditional or loop structures.
* No exceptions.
* Instance is always immutable.
* Instances are created by class name, however method always operate with types.

## Syntax
### HelloWorld.mi

```csharp
// Declaration of modules which are used by this type.
// Tabulation identifies nested modules.
uses Global\System
  IO
  Collections
  
// Declaration of base types in format: is Type1, Type2, etc.
is Program

// Methods and functions.

// Runs the program. This method is declared in the Program type.
run(arguments Array<String>)
  // Creates instance of a Console type, then calls writeLine method.
  Console().writeLine('Hello World!')
```

## The Name
Miri is named after my daughter. It’s her nickname in our family. Her short name is Mira and the full name is Myroslava.
When she was about 6 month I saw StarTrek for the first time in my life. There was a girl Miri. We already called our daughter like that, so this is just funny coincidence.
Using girl’s name for programming language doesn’t bother me at all. Things should have value beyond their names. Same applies to people.
