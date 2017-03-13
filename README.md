# M: pure object-oriented language
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

namespace Primitives
  // The base type for any numeric object.
  type Numeric extends Object
    add(right Numeric) Numeric
    sub(right Numeric) Numeric
    neg() Numeric
    mul(right Numeric) Numeric
    div(right Numeric) Numeric
    mod(right Numeric) Numeric
    
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

```
namespace Actors
  class ComplexActor implements Actor
    (street Bool) Actor
      _street = street

    act()
      Bool(_street)
        .match(True, dance)
        .match(False, sing)

    dance()
      Console().write('I am dancing')
    
    sing()
      Console().write('I am singing')

ComplexActor(street).act()
```
