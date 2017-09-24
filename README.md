# Miri: pure object-oriented language
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
