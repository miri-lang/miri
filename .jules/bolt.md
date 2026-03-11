## 2024-03-03 - [AST Cloning Overhead in Pipeline]
**Learning:** During the initial pipeline phase `wrap_script_in_main`, deep clones of entire AST statement nodes were occurring simply to split the top-level items from executable items.
**Action:** When filtering or splitting a vector of owned AST nodes (e.g. `program.body`), use a two-pass approach: first verify valid types by reference, then use `std::mem::take` to consume the vector and `push` directly into new vectors. This completely avoids `.clone()` on AST nodes.

## 2024-03-03 - [Eliminate String Clones During AST Node Extraction]
**Learning:** Functions like `extract_name` and `extract_type_name` in the type checker often extracted Strings by cloning them off AST identifiers just for lookup checks. This led to thousands of needless allocations per pass. Wait... actually I should make sure my PR details this. Returning `&str` requires careful lifetime handling if those names are later inserted into a HashMap that expects owned Strings, but many reads do not need ownership.
**Action:** When writing extraction helpers (like `extract_name`), use lifetimes `Result<&'a str, String>` tying the return value to the AST node instead of returning `Result<String, String>`. Callers that need ownership can call `.to_string()` themselves, while callers only needing comparisons or temporary lookups skip the heap allocation entirely.

## 2024-03-03 - [Target Hot Paths Instead of Cold Paths for Optimization]
**Learning:** Optimizing cold paths (like deduplication logic in error reporting `reported_errors`) goes against the principle of avoiding micro-optimizations that have no measurable impact. Always look for hot paths (e.g., `is_auto_copy_inner`) that are executed continuously. Replacing a `String` clone with a string slice borrow (`&'a str`) in hot paths provides a measurable performance gain.
**Action:** When searching for optimizations, ignore error reporting logic entirely. Focus on parsing, type checking traversals, cycle detection (`is_auto_copy`), and codegen translation where operations are executed frequently and optimizations yield tangible benefits.

## 2024-05-18 - [Eliminate format! overhead in hot parsing paths]
**Learning:** Using the `format!` macro for simple string concatenation (like `format!("{}::{}", class, id)`) in hot loops like `identifier_to_type_name` introduces significant runtime overhead. This overhead comes from parsing the format string and setting up the internal `fmt::Arguments` machinery, which forces unnecessary heap allocations compared to exact manual calculation. Micro-benchmarking showed that `push_str` with pre-allocated capacity is ~5.5x faster in Rust.
**Action:** Always pre-calculate required string capacities (`len1 + len2 + etc`) and use `String::with_capacity` followed by `push_str` when concatenating strings sequentially in compiler hot loops. Avoid `format!` unless complex formatting (e.g., numbers, padding) is explicitly needed.
