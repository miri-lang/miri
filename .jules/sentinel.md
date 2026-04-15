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
## 2024-05-27 - [Path Traversal in Dynamic Binary Execution]
**Vulnerability:** Symlink attacks and path traversal vulnerabilities existed when executing dynamically built binaries from temporary directories in `src/pipeline.rs`.
**Learning:** Checking whether a path resides within a given base directory using simple string matching or naive `starts_with` is insufficient if symbolic links or uncanonicalized relative directories (`..`) are involved. This could allow an attacker to bypass the containment check and execute an arbitrary binary via `Command::new`.
**Prevention:** Always use `canonicalize()` on both the base directory and the target executable path to resolve symlinks and relative references *before* performing containment checks like `canonical_executable.starts_with(&canonical_temp)`.
## 2025-02-16 - Safe Memory Allocation in Core Data Structures

**Vulnerability:** Integer overflow causing undersized allocations and heap buffer overflows (OOM DoS) in MiriList.
**Learning:** `capacity * elem_size` without bounds checks wrappers can lead to extremely large inputs wrapping around, producing a small layout that receives too much data during memory copies or inserts.
**Prevention:** Always use safe arithmetic like `checked_mul` (e.g., `capacity.checked_mul(elem_size)`) and return gracefully or use safe aborts if the required size overflows before calling `Layout::from_size_align` in manual memory management code. Ensure fallback paths (like returning an empty struct) are maintained if the checked math fails.
