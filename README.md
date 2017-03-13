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
  // Everything comes from Void.
  type Void
  
namespace Primitives
  type Object extends Void

namespace Primitives
  // The base type for any numeric object.
  type Numeric extends Object
    Numeric add(Numeric right)
    Numeric sub(Numeric right)
    Numeric neg
    Numeric mul(Numeric right)
    Numeric div(Numeric right)
    Numeric mod(Numeric right)
    
namespace Enumerables
  type EnumerableNumeric excends Object
    Void times(Expression expression)
    
namespace Primitives
  // The integer number.
  class Int implements Numeric
    Numeric (Numeric value)
      _value = value
      
    Numeric add(Numeric right)
      _value + right
      ...
```

```
sum = Int(10)
5.times(i => sum = sum.add(10))
```

```
namespace Actors
  class ComplexActor implements Actor:
    Actor (Bool street)
      _street = street

    Void act
      Bool(_street)
        .match(True, dance)
        .match(False, sing)

    Void dance
      Console().write('I am dancing')
    
    Void sing
      Console().write('I am singing')

ComplexActor(street).act()
```
