## 2024-03-03 - [AST Cloning Overhead in Pipeline]
**Learning:** During the initial pipeline phase `wrap_script_in_main`, deep clones of entire AST statement nodes were occurring simply to split the top-level items from executable items.
**Action:** When filtering or splitting a vector of owned AST nodes (e.g. `program.body`), use a two-pass approach: first verify valid types by reference, then use `std::mem::take` to consume the vector and `push` directly into new vectors. This completely avoids `.clone()` on AST nodes.
