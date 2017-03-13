# M: pure object-oriented language
## Syntax ideas
```
namespace Actors: 
  class ComplexActor implements Actor:
    Tag (Bool street):
      _street = street

    Void Act:
      If(_street)
        .Then(dance)
        .Else(sing)

    Void Dance:
      Console().Write('I am dancing')
    
    Void Sing:
      Console().Write('I am singing')

ComplexActor(street).Act()
```
