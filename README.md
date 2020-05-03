# Miri: General Purpose Object-Oriented Language

## Features
* Static typing.
* Folder structure and file names play key role in namespacing:
  * Folder name is part of a namespace.
  * File name is also a type name. Full type name example: `Blog/Users/User` corresponds to `Blog/Users/User.mi` file.
  * One file describes one type only.
* No variables. Data structures are immutable.
* Instance fields are prefixed with underscode: `_field`.
* Instance variables can’t change.
* Supports generics types.
* No null/nil reference.
* Objects can’t be instantiated inside classes.

## Examples

### Classic Hello World (simple version)

#### HelloWorldApp/Program.miri

```ruby
use /system/programs

extend ConsoleProgram

run ExitCode
  _console.writeLine 'Hello World!'
  ExitCode.default
```

### Classic Hello World (extended version)

This version also includes unit-test, which in Miri is part of the same function description.

#### HelloWorldApp/Program.miri

```ruby
use /system/programs
use /system/types
use /system/io/fakes

extend ConsoleProgram

// Test constructor
new(:test, args []String) ThisType
  new(FakeConsole.new, args)
  
// Global examples section
examples
  setup
    _subject = new(:test, []String.new)

// runs the program.
run ExitCode
  _console.writeLine 'Hello World!'
  ExitCode.default

  // Local examples section
  examples
    example 'with default params'
      expect _subject.run == ExitCode.default

      it 'outputs "Hello World!" to console'
        _subject.run
        expect _console.buffer.contains?('Hello World!')
```
