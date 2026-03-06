## 2024-03-03 - [AST Cloning Overhead in Pipeline]
**Learning:** During the initial pipeline phase `wrap_script_in_main`, deep clones of entire AST statement nodes were occurring simply to split the top-level items from executable items.
**Action:** When filtering or splitting a vector of owned AST nodes (e.g. `program.body`), use a two-pass approach: first verify valid types by reference, then use `std::mem::take` to consume the vector and `push` directly into new vectors. This completely avoids `.clone()` on AST nodes.

## 2024-03-03 - [Eliminate String Clones During AST Node Extraction]
**Learning:** Functions like `extract_name` and `extract_type_name` in the type checker often extracted Strings by cloning them off AST identifiers just for lookup checks. This led to thousands of needless allocations per pass. Wait... actually I should make sure my PR details this. Returning `&str` requires careful lifetime handling if those names are later inserted into a HashMap that expects owned Strings, but many reads do not need ownership.
**Action:** When writing extraction helpers (like `extract_name`), use lifetimes `Result<&'a str, String>` tying the return value to the AST node instead of returning `Result<String, String>`. Callers that need ownership can call `.to_string()` themselves, while callers only needing comparisons or temporary lookups skip the heap allocation entirely.
