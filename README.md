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

## Example

### HelloWorldApp/Program.miri

```ruby
uses Global/System
  IO
  Collections
  
is Program

// Constructor.
new(console Console)
  _console = console
  
// Runs the program.
run
  _console.writeLine('Hello World!')
```

### HelloWorldApp/Program.test.miri

```ruby
uses Global/System/IO/Fakes
  
extends UnitTest

new
  _console = FakeConsole.new
  _subject = Program.new(_console)

run
  context 'with default params'
    it 'outputs "Hello World!" to console'
      _subject.run
      expect(_console.containsInBuffer('Hello World!)).to be_true
```
