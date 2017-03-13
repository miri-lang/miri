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
  type TObject

namespace Primitives
  // The base type for any numeric object.
  type TNumeric extends TObject
    TNumeric add(TNumeric right)
    TNumeric sub(TNumeric right)
    TNumeric neg
    TNumeric mul(TNumeric right)
    TNumeric div(TNumeric right)
    TNumeric mod(TNumeric right)
    
namespace Enumerables
  type TEnumerableNumeric excends TObject
    TVoid times(TExpression expression)
    
namespace Primitives
  // The integer number.
  class Int implements TNumeric
    TNumeric (TNumeric value)
      _value = value
      
    TNumeric add(TNumeric right)
      _value + right
      ...
```

```
sum = Int(10)
5.times(i => sum = sum.add(10))
```

```
namespace Actors
  class ComplexActor implements TActor:
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
