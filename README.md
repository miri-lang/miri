# Miri: General Purpose Object-Oriented Language
## Goal
Combine good practices of object-oriented design and enforce them on programming language level. The final product should satisfy definition of a modern general purpose programming language.

## Motivation
I believe large amount of books and articles on “good software design” is a result of excessive flexibility of modern programming languages. What is the point of giving a knife to a child and then attempting to teach them how to use the knife, after so many cuts?
Human mind has many faults which result into poor software quality. Experience and knowledge increase software quality, but there’s no easy way to ensure constant delivery of high quality code. Developers get boring, lazy, loose motivation. Product managers don’t really understand why we need “refactoring”.
The thing is: modern programming languages allow us to cut corners and create bad software. They allow us to act bad. It is that kind of democracy where killing or robbing people is lawful, but frouned upon.
We all agree such terrible acts should be forbidden by law. Why do we have programming languages which OK with us being bad and then use all kinds of poLINTsmen to control our actions?
Let us create proper democracy, where bad practices are forbidden. `Miri` will be your linter, cop, guide and book on good software design. If you use this language, you’ll have no chance to do wrong.

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

## Syntax ideas
### Primitives
```
namespace Primitives
  // The most generic type which is the base to all other types.
  // Everything comes from the Void.
  type Void
  
namespace Primitives
  type Object extends Void
    is(expected Object) Bool
  
namespace Primitives
  type Comparable extends Void
    match(expected Comparable, expression Expression) Comparable

namespace Primitives
  // The base type for any numeric object.
  type Numeric extends Object, Comparable
    add(right Numeric) Numeric
    sub(right Numeric) Numeric
    neg() Numeric
    mul(right Numeric) Numeric
    div(right Numeric) Numeric
    mod(right Numeric) Numeric
    match(expected Numeric, expression Expression) Numeric
    
namespace Enumerables
  use Primitives
  type EnumerableNumeric extends Object
    times(expression Expression)
    
namespace Primitives
  // The integer number.
  class Int implements Numeric
    (value Numeric) Numeric
      _value = value
      
    add(right Numeric) Numeric
      _value + right
      ...
```

```
sum = Int(10)
5.times(i => sum = sum.add(10))
```

#### Boolean
```
5.is(Numeric)
  True => Console().write('5 is numeric'))
  False => Console().write('5 is not numeric'))
5.is_not(Numeric) => Console().write('5 is numeric')
console = Console()
'hello'.match()
  'Hello' => console.write('matches \'Hello\'')
  'world' => console.write('matches \'world\')
```

### Examples
```
namespace Actors
  class ComplexActor implements Actor
    (street Bool) Actor
      _street = street

    act()
      _street
        .match(True, dance)
        .match(False, sing)

    dance()
      Console().write('I am dancing')
    
    sing()
      Console().write('I am singing')

ComplexActor(street).act()
```
