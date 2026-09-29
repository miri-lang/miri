## Rule

A method of a generic class was called at an instance of that class its obligations were never checked at. The compiler compiles a method only at the instances where its requirements on the class's type arguments were proven, and fills every other place the method could be reached from — a vtable slot, a set's or map's comparison — with a trap that stops the program here. Reaching it means the type checker let through a call it should have refused, which is a compiler bug: the program stops with this code rather than calling code compiled for no type.

## Before

This error has no source-level reproduction: a program that reaches it is one the type checker should have refused.

## After

Report it with the smallest program that reaches it:

```sh
miri run program.mi
```

As a workaround, annotate the value's type where it is first built, with every type argument written out, so the method's requirements are checked at the call.

## Reference

[Runtime Errors and Traps](../reference/runtime.md)
