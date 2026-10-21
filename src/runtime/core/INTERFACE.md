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
          [elem_clone_fn: usize][elem_order_kind: usize][elem_compare_fn: usize]
```

`data` points to a separately-allocated zeroed buffer of `elem_count * elem_size` bytes.
The element count is fixed at creation. `elem_drop_fn`, when non-zero, is called on
each element pointer (read as a pointer-sized word) when the array is freed so that
managed elements have their RC decremented.

#### MiriList (`list.rs`)

```
[RC: usize][data: *mut u8][len: usize][capacity: usize][elem_size: usize][elem_drop_fn: usize]
          [elem_clone_fn: usize][elem_order_kind: usize][elem_compare_fn: usize]
```

`data` points to a separately-allocated buffer that grows by doubling. `elem_drop_fn`,
when non-zero, is called on each removed element pointer to decrement its RC.

Both sortable containers carry an element order (`element_order.rs`): a kind
saying whether `sort` reads an element's bytes as a signed, unsigned or float
value, and a comparator that replaces that reading for elements whose bytes are
a reference. Codegen registers both from the element type; `clone` and every
list copied from an array keep them.

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

#### MiriString (`string/core.rs`)

```
[RC: usize][data: *mut u8][len: usize][capacity: usize][ascii: u8][storage: u8][block_room: u8][padding]
          [inline bytes: block_room]
```

Compiled code reads `len` one word into the payload, and writes every string
literal with this layout: an immortal RC, `capacity` equal to `len`, and the rest
of the `ascii` word zeroed. `storage` says where the bytes live and what freeing
the string releases:

| `storage` | Bytes | Released by `miri_rt_string_free` |
|-----------|-------|------------------------------------|
| `0` unowned | static data such as a literal's | never |
| `1` heap | a `miri_alloc` buffer of `capacity` bytes (null when empty) | the buffer, then the block |
| `2` inline | `capacity` bytes right after the header, in the same block | the block |

A string of up to `INLINE_CAPACITY` bytes (216 on a 64-bit target) is made inline,
so the whole block — RC word, header and bytes — fits the block cache's largest
class and costs one cached block and no data buffer. `block_room` records how many
bytes the block holds past the header; the block is always released at the header
size plus `block_room`, which stays right after an append has moved inline bytes
into a heap buffer. An append to a string that is not heap-backed first copies its
bytes into a heap buffer of its own.

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
- Small blocks — up to 256 bytes, both `[RC][payload]` blocks and the blocks
  compiled code lays out itself — come from a per-thread cache of released blocks
  (`block_cache`) in front of the system allocator. A block it hands out holds
  whatever its previous owner left there, so every constructor writes all of its
  fields. While `MIRI_HEAP_GUARD=1` is set the cache is bypassed, so the guard sees
  every block go back to the system.
- A parked block's first word links it to the next and its second holds a
  fixed sentinel no live block holds there. Releasing a block that already carries it (a double
  release), or taking one whose sentinel changed (a write after release), traps
  with `MER_RT_015` in every build.
- **Poison mode.** In a build without `--release`, while `MIRI_LEAK_CHECK`,
  `MIRI_ALLOC_COUNT` or `MIRI_RC_STATS` is asked for or `MallocScribble` is set
  — so in every test run — the cache also fills each released block past those
  two words with `0xDD`, holds it back behind 64 others of its class, checks the
  fill when the block is handed out (a difference traps with `MER_RT_015`), fills
  every handed-out block with `0xAA`, and refuses a release that names a size
  class larger than the block the system gave. A release build is never
  poisoned.
- Parked memory is per thread, at most 16 MiB each, so a process can hold its
  thread count times that. `MIRI_ALLOC_COUNT` also prints
  `MIRI_ALLOC_REUSED: <n> block(s) handed out again from released ones`.

---

## Stable FFI Interface

The stable interface is defined by the `pub mod ffi` submodule inside each runtime
module. Only functions in `pub mod ffi` are part of the public ABI. Internal helpers
at module level are private and may change without notice.

| Module       | `pub mod ffi` location      | Key symbols                                        |
|--------------|-----------------------------|----------------------------------------------------|
| `alloc`      | `alloc::ffi`                | `miri_alloc`, `miri_alloc_zeroed`, `miri_realloc`, `miri_free` |
| `block_cache`| `block_cache::ffi`          | `miri_rt_block_alloc`, `miri_rt_block_free`        |
| `array`      | `array::ffi`                | `miri_rt_array_new`, `miri_rt_array_free`, `miri_rt_array_len`, … |
| `list`       | `list::ffi`                 | `miri_rt_list_new`, `miri_rt_list_free`, `miri_rt_list_len`, … |
| `set`        | `set::ffi`                  | `miri_rt_set_new`, `miri_rt_set_free`, `miri_rt_set_len`, … |
| `map`        | `map::ffi`                  | `miri_rt_map_new`, `miri_rt_map_free`, `miri_rt_map_len`, … |
| `io`         | `io::ffi`                   | `miri_rt_print`, `miri_rt_println`, `miri_rt_eprint`, `miri_rt_eprintln`, `miri_rt_get_line_end` |
| `string`     | `string::ffi`               | `miri_rt_string_new`, `miri_rt_string_free`, `miri_rt_string_len`, … |
| `time`       | `time::ffi`                 | `miri_rt_nanotime`, `miri_rt_sleep_nanos`         |
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
   `make check-runtime-fns` target will catch any mismatch between declared names and
   exported symbols.

4. **Add a `.mi` stdlib file** under `src/stdlib/` that exposes the new type to Miri
   programs. Declare FFI-backed methods with `runtime "core" fn miri_rt_{name}_{op}(...)`
   so the compiler generates the correct call sites.
