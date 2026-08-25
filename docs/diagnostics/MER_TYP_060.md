## Rule

An `async` or `await` keyword was used in an invalid context. These keywords are reserved for asynchronous function definitions and expressions; `async` functions cannot be used where synchronous code is required, and `await` can only appear inside `async` functions.

## Before

```miri
fn process() i32:
  let result = await async_operation()
  0
```

## After

```miri
async fn process() i32:
  let result = await async_operation()
  0
```

## Reference

[Type Checker](../reference/types.md)
