# Miri Runtime Core — Interface Contract

This document defines the stable contract between the Miri compiler and the runtime
library. All symbols listed here are exported with `#[no_mangle] extern "C"` and are
guaranteed to be stable across patch releases.

---

## Naming Convention

```
miri_rt_{type}_{operation}
```

All lowercase, words separated by underscores. The type segment identifies the runtime
type (e.g. `array`, `list`, `set`, `map`, `string`, `tuple`). The operation segment
describes what the function does.

**Exception**: allocator functions use the prefix `miri_alloc` (no `_rt_` segment) to
match standard allocator naming:

- `miri_alloc(size, align) -> *mut u8`
- `miri_alloc_zeroed(size, align) -> *mut u8`
- `miri_realloc(ptr, old_size, align, new_size) -> *mut u8`
- `miri_free(ptr, size, align)`

---

## Memory Layout

### RC-managed heap types

All heap-allocated Miri objects (arrays, lists, sets, maps, class instances) share
the layout:

```
[RC: usize][payload]
```

The pointer stored in a Miri variable points **past the RC header** to the start of the
payload. Field offsets within the payload are therefore the same regardless of the RC
header. The RC header is at `ptr - RC_HEADER_SIZE` (8 bytes on 64-bit platforms).

#### MiriArray (`array.rs`)

```
[RC: usize][data: *mut u8][elem_count: usize][elem_size: usize][elem_drop_fn: usize]
```

`data` points to a separately-allocated zeroed buffer of `elem_count * elem_size` bytes.
The element count is fixed at creation. `elem_drop_fn`, when non-zero, is called on
each element pointer (read as a pointer-sized word) when the array is freed so that
managed elements have their RC decremented.

#### MiriList (`list.rs`)

```
[RC: usize][data: *mut u8][len: usize][capacity: usize][elem_size: usize][elem_drop_fn: usize]
```

`data` points to a separately-allocated buffer that grows by doubling. `elem_drop_fn`,
when non-zero, is called on each removed element pointer to decrement its RC.

#### MiriSet (`set.rs`)

```
[RC: usize][data: *mut u8][len: usize][states: *mut u8][capacity: usize][elem_size: usize]
```

Open-addressing hash set with linear probing. `states` is a byte array with values
`0=EMPTY`, `1=OCCUPIED`, `2=TOMBSTONE`. The first two fields (`data`, `len`) intentionally
match `MiriList`/`MiriArray` so that `Rvalue::Len` and `element_at` work uniformly.

#### MiriMap (`map.rs`)

```
[RC: usize][states: *mut u8][keys: *mut u8][values: *mut u8][len: usize][capacity: usize]
          [key_size: usize][value_size: usize][key_kind: usize][val_drop_fn: usize]
```

Open-addressing hash map. `key_kind`: `0` = value type (int/float/bool, compared by
raw bytes), `1` = string type (dereferences `MiriString` pointer for hash/compare).
`val_drop_fn`, when non-zero, is called on each removed value pointer to decrement its RC.

### Non-RC heap types

#### MiriString (`string/core.rs`)

```
[data: *mut u8][len: usize][capacity: usize]
```

`MiriString` does **not** have an RC header. It is Box-allocated by the runtime and freed
via `miri_rt_string_free`. The pointer stored in a Miri variable points directly to the
`MiriString` struct.

---

## Ownership Model

- All heap-allocated objects start with `RC = 1` when created by an `_new` function.
- The Miri compiler emits explicit IncRef/DecRef instructions (Cranelift `call`s to
  the generated RC helpers).
- Runtime FFI functions do **not** IncRef their pointer arguments unless explicitly
  documented. Callers must IncRef before passing a pointer if they want to retain
  ownership.
- When RC drops to zero, the type's `_free` function is responsible for freeing the
  internal buffers and then the `[RC][payload]` block.

---

## Stable FFI Interface

The stable interface is defined by the `pub mod ffi` submodule inside each runtime
module. Only functions in `pub mod ffi` are part of the public ABI. Internal helpers
at module level are private and may change without notice.

| Module       | `pub mod ffi` location      | Key symbols                                        |
|--------------|-----------------------------|----------------------------------------------------|
| `alloc`      | `alloc::ffi`                | `miri_alloc`, `miri_alloc_zeroed`, `miri_realloc`, `miri_free` |
| `array`      | `array::ffi`                | `miri_rt_array_new`, `miri_rt_array_free`, `miri_rt_array_len`, … |
| `list`       | `list::ffi`                 | `miri_rt_list_new`, `miri_rt_list_free`, `miri_rt_list_len`, … |
| `set`        | `set::ffi`                  | `miri_rt_set_new`, `miri_rt_set_free`, `miri_rt_set_len`, … |
| `map`        | `map::ffi`                  | `miri_rt_map_new`, `miri_rt_map_free`, `miri_rt_map_len`, … |
| `io`         | `io::ffi`                   | `miri_rt_print`, `miri_rt_println`, `miri_rt_eprint`, `miri_rt_eprintln`, `miri_rt_get_line_end` |
| `string`     | `string::ffi`               | `miri_rt_string_new`, `miri_rt_string_free`, `miri_rt_string_len`, … |
| `time`       | `time::ffi`                 | `miri_rt_nanotime`                                 |
| `tuple`      | `tuple::ffi`                | `miri_rt_tuple_len`                                |

All symbols are also re-exported at the crate root via `pub use {module}::ffi::*` in
`lib.rs` so that both `crate::miri_rt_list_new()` and `miri_runtime_core::miri_rt_list_new()`
resolve correctly.

The `rc` module contains only internal helpers (`alloc_with_rc`, `free_with_rc`,
`RC_HEADER_SIZE`) and does **not** expose a `pub mod ffi`.

---

## New Module Checklist

Follow these four steps when adding a new runtime type:

1. **Create `src/runtime/core/src/{name}.rs`**: define the struct (with `#[repr(C)]`),
   its impl block, private helpers, and a `pub mod ffi { ... }` containing all
   `#[no_mangle] extern "C"` functions.

2. **Register in `lib.rs`**: add `pub mod {name};` and `pub use {name}::ffi::*;` so the
   symbols are accessible at the crate root and visible to the linker.

3. **Add constants to `src/runtime_fns.rs`** (compiler side): declare each new symbol
   name as a `pub const` so the MIR lowering and codegen can reference it by name. The
   Phase 4 drift-check (`make check-runtime-fns`) will catch any mismatch between
   declared names and exported symbols.

4. **Add a `.mi` stdlib file** under `src/stdlib/` that exposes the new type to Miri
   programs. Declare FFI-backed methods with `runtime "core" fn miri_rt_{name}_{op}(...)`
   so the compiler generates the correct call sites.
