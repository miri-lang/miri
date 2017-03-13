# M: pure object-oriented language
## Syntax ideas
```
namespace Actors: 
  class ComplexActor implements TActor:
    Actor (Bool street):
      _street = street

    Void act:
      Bool(_street)
        .match(True, dance)
        .match(False, sing)

    Void dance:
      Console().write('I am dancing')
    
    Void sing:
      Console().write('I am singing')

ComplexActor(street).act()
```
