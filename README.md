# Miri: General Purpose Object-Oriented Language

We, engineers, have been balancing speed and quality over decades. When we want to create a prototype very fast, we achieve that by using a less verbose language, preferably without any intermediate steps like compilation or packaging. When we value speed of execution, we resort to extremes of writing and using whatever runs faster. Finally, when we focus on quality of software, we tend to choose languages and methods more verbose and complex, like static typing, design patterns etc.
Sometimes quality and speed are good friends: when your code is good, you can extend the software faster. Other times we need to compromise, because a language designed to be fun and pleasant to use in local environment, may not be that fun to run on high-loaded production system.
Do we always need to seek those compromises? Is it possible to delegate some of those choices to a programming language and have a balanced speed/quality, while focusing on creation?
Balance of speed and quality is the core philosophy of Miri. It’s designed to make software engineering fun and productive, but not at the cost of quality and performance.

## Features

* Static typing.
* Compileable. Tests are part of the compilation.
* Folder structure and file names play key role in namespacing:
  * Folder name is part of a namespace.
  * File name is also a type name. Full type name example: `Blog/Users/User` corresponds to `Blog/Users/User.mi` file.
  * One file describes one type only.
* No variables. Data structures are immutable.
* Instance fields are prefixed with underscode: `_field`.
* Instance variables can’t change.
* Supports generics types.
* No null/nil reference.

## Examples

### Classic Hello World (simple version)

#### HelloWorldApp/App.miri

```ruby
/sys/apps

extends ConsoleApp

run
  out 'Hello World!'
  ExitCode.default
```

### Classic Hello World (extended version)

This version also includes unit-test, which in Miri is part of the same function description.

#### HelloWorldApp/App.miri

```ruby
/sys/apps

extends ConsoleApp

# Fake constructor, used in the examples.
new
  new(Console.fake, []String.new)

# runs the app.
run
  out 'Hello World!'
  ExitCode.default

  test
    fake.run == ExitCode.default
    fake.run
      buffer.contains? 'Hello World!'
```
