## 2024-05-24 - [Compiler DoS via Unexpected EOF on Generic Syntax]
**Vulnerability:** A panic (`unwrap()`) in the parser's generic arguments handling (`call_member_expression()`) triggered when reaching an unexpected end-of-file.
**Learning:** `Option::unwrap()` is frequently unsafe around lookahead buffers. Compilers must gracefully handle truncated input at any token stream boundary to avoid DoS.
**Prevention:** Use pattern matching (`if let Some(...) = self._lookahead`) on lookahead variables and break parsing loops safely on `None` (EOF).

## 2024-05-25 - [Compiler DoS via Unchecked Indent Stack Unwrap]
**Vulnerability:** The lexer panics (`unwrap()`) when attempting to handle unclosed indentation structures because it assumes the `indent_stack` will never be empty while dedenting.
**Learning:** Compilers must not trust their own internal state tracking blindly when processing untrusted input streams. An unexpected EOF or malformed whitespace can deplete stacks faster than expected.
**Prevention:** Use safe pattern matching (e.g., `while let Some(...) = stack.last()`) and break cleanly when stacks are depleted during whitespace processing.
## 2024-05-15 - Unhandled Error in Drop
**Vulnerability:** `unwrap()` inside `Drop` implementations in core runtime types (`MiriList`, `MiriArray`).
**Learning:** `unwrap()` in `Drop` is particularly dangerous. If the `unwrap()` panics during unwinding from another panic, it causes an immediate abort of the process (double panic). For memory allocations, an invalid layout can panic the program instead of failing gracefully.
**Prevention:** Avoid panicking (`unwrap`, `expect`) inside `Drop` handlers. Gracefully swallow errors if there is no way to propagate them, especially since `Drop` cannot return a `Result`.

## 2024-05-26 - [Integer Overflow UB in Layout Calculation]
**Vulnerability:** Potential Integer Overflow during capacity calculations in `free_buffers` of `src/runtime/core/src/map.rs` where `capacity * key_size` is calculated.
**Learning:** Raw memory deallocation operations using `Layout::from_size_align` can accept incorrect wrapped-around integer values resulting in Undefined Behavior (UB) if the size overflows `usize`.
**Prevention:** Use `checked_mul` (e.g. `capacity.checked_mul(key_size)`) to detect overflow during manual memory management operations.

## 2024-05-27 - [Symlink/Path Traversal in Temp Execution]
**Vulnerability:** The compiler invoked built executables from a temp directory via `Command::new(executable_path)`. If `executable_path` was a relative path navigating outside the temp dir, or a symlink to another host binary, it could lead to arbitrary command execution on the host machine during the build phase.
**Learning:** `Command::new` follows paths naively. When executing artifacts generated from untrusted source in a temporary directory, strict containment checks must be enforced.
**Prevention:** Always use `canonicalize()` to resolve both the temporary directory and the target executable path, and perform a strict containment check (`canonical_exe.starts_with(&canonical_temp)`) to ensure the executable is genuinely contained within the safe environment before execution.
