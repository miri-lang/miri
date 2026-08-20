// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Runtime function name constants.
//!
//! Centralizes every `miri_rt_*` symbol name so that renaming a runtime
//! function requires changing exactly one place in the compiler rather than
//! hunting down scattered string literals.
//!
//! # Naming convention
//!
//! All runtime symbols follow the pattern `miri_rt_{type}_{operation}`,
//! all lowercase.  They are exported from the runtime static library as
//! `#[no_mangle] pub extern "C"` functions.
//!
//! Examples: `miri_rt_list_push`, `miri_rt_string_len`, `miri_rt_map_clear`.
//!
//! # Drift prevention
//!
//! Every symbol declared as `runtime "core" fn` in a stdlib `.mi` file must
//! have a matching constant in [`rt`].  The test in
//! `tests/stdlib/runtime_fns_sync.rs` enforces this automatically.

/// Constants for all `miri_rt_*` runtime symbols.
pub mod rt {
    // ── Closure ──────────────────────────────────────────────────────────────
    /// Compiler-internal: increments CLOSURE_ALLOC_BALANCE on closure malloc, not in stdlib.
    pub const CLOSURE_ALLOC_TRACK: &str = "miri_rt_closure_alloc_track";
    /// Compiler-internal: decrements CLOSURE_ALLOC_BALANCE on closure free, not in stdlib.
    pub const CLOSURE_FREE_TRACK: &str = "miri_rt_closure_free_track";
    /// Test-only: simulates a closure leak to verify the MIRI_LEAK_CHECK detector.
    pub const CLOSURE_SIMULATE_LEAK: &str = "miri_rt_test_simulate_closure_leak";
    /// Test-only: frees one allocation twice to verify the heap guard's
    /// double-free trap.
    pub const SIMULATE_DOUBLE_FREE: &str = "miri_rt_test_simulate_double_free";
    /// Compiler-internal: registers an inline `malloc` with the heap guard, not
    /// in stdlib. Covers class instances, tuples, Options, enum payloads and
    /// closure environments, which codegen allocates without the runtime.
    pub const CLASS_ALLOC_TRACK: &str = "miri_rt_class_alloc_track";
    /// Compiler-internal: witnesses an inline `free` for the heap guard, not in
    /// stdlib. Paired with [`CLASS_ALLOC_TRACK`].
    pub const CLASS_FREE_TRACK: &str = "miri_rt_class_free_track";
    /// Compiler-internal: runtime byte telling compiled code whether either
    /// tracking hook above is worth calling. A data symbol, not a function —
    /// codegen loads it and branches, so an unobserved allocation pays a load
    /// rather than a call it cannot inline.
    pub const TRACKING_STATE: &str = "miri_rt_tracking_state";

    // ── Array ────────────────────────────────────────────────────────────────
    pub const ARRAY_NEW: &str = "miri_rt_array_new";
    pub const ARRAY_FREE: &str = "miri_rt_array_free";
    pub const ARRAY_LEN: &str = "miri_rt_array_len";
    pub const ARRAY_SET_VAL: &str = "miri_rt_array_set_val";
    pub const ARRAY_SORT: &str = "miri_rt_array_sort";
    pub const ARRAY_CLONE: &str = "miri_rt_array_clone";
    /// Compiler-internal: partial readback of `g.slice(range)`, not in stdlib.
    pub const ARRAY_SLICE: &str = "miri_rt_array_slice";
    /// Compiler-internal: bounds-check panic helper, not declared in stdlib.
    pub const ARRAY_PANIC_OOB: &str = "miri_rt_array_panic_oob";
    /// Compiler-internal: decrements the RC of an array element, not in stdlib.
    pub const ARRAY_DECREF_ELEMENT: &str = "miri_rt_array_decref_element";
    /// Compiler-internal: registers the element drop function on an array, not in stdlib.
    pub const ARRAY_SET_ELEM_DROP_FN: &str = "miri_rt_array_set_elem_drop_fn";
    /// Compiler-internal: registers the element clone function on an array, not in stdlib.
    pub const ARRAY_SET_ELEM_CLONE_FN: &str = "miri_rt_array_set_elem_clone_fn";

    // ── Tuple ─────────────────────────────────────────────────────────────────
    pub const TUPLE_LEN: &str = "miri_rt_tuple_len";

    // ── List ─────────────────────────────────────────────────────────────────
    pub const LIST_NEW: &str = "miri_rt_list_new";
    pub const LIST_FREE: &str = "miri_rt_list_free";
    pub const LIST_LEN: &str = "miri_rt_list_len";
    pub const LIST_PUSH: &str = "miri_rt_list_push";
    pub const LIST_POP: &str = "miri_rt_list_pop";
    pub const LIST_SET: &str = "miri_rt_list_set";
    pub const LIST_INSERT: &str = "miri_rt_list_insert";
    pub const LIST_REMOVE: &str = "miri_rt_list_remove";
    pub const LIST_TAKE_AT: &str = "miri_rt_list_take_at";
    pub const LIST_CLEAR: &str = "miri_rt_list_clear";
    pub const LIST_REVERSE: &str = "miri_rt_list_reverse";
    pub const LIST_SORT: &str = "miri_rt_list_sort";
    pub const LIST_IS_EMPTY: &str = "miri_rt_list_is_empty";
    pub const LIST_CLONE: &str = "miri_rt_list_clone";
    /// Compiler-internal: Copy-on-Write check before mutation, not in stdlib.
    pub const LIST_COW: &str = "miri_rt_list_cow";
    /// Compiler-internal: constructs a list from a raw pointer, not in stdlib.
    pub const LIST_NEW_FROM_RAW: &str = "miri_rt_list_new_from_raw";
    /// Compiler-internal: constructs a list from a managed array, not in stdlib.
    pub const LIST_NEW_FROM_MANAGED_ARRAY: &str = "miri_rt_list_new_from_managed_array";
    /// Compiler-internal: decrements the RC of a list element, not in stdlib.
    pub const LIST_DECREF_ELEMENT: &str = "miri_rt_list_decref_element";
    /// Compiler-internal: registers the element drop function, not in stdlib.
    pub const LIST_SET_ELEM_DROP_FN: &str = "miri_rt_list_set_elem_drop_fn";
    /// Compiler-internal: registers the element clone function, not in stdlib.
    pub const LIST_SET_ELEM_CLONE_FN: &str = "miri_rt_list_set_elem_clone_fn";

    // ── Map ──────────────────────────────────────────────────────────────────
    pub const MAP_NEW: &str = "miri_rt_map_new";
    pub const MAP_FREE: &str = "miri_rt_map_free";
    pub const MAP_LEN: &str = "miri_rt_map_len";
    pub const MAP_IS_EMPTY: &str = "miri_rt_map_is_empty";
    pub const MAP_SET: &str = "miri_rt_map_set";
    pub const MAP_GET: &str = "miri_rt_map_get";
    pub const MAP_CONTAINS_KEY: &str = "miri_rt_map_contains_key";
    pub const MAP_REMOVE: &str = "miri_rt_map_remove";
    pub const MAP_CLEAR: &str = "miri_rt_map_clear";
    pub const MAP_KEY_AT: &str = "miri_rt_map_key_at";
    pub const MAP_VALUE_AT: &str = "miri_rt_map_value_at";
    /// Compiler-internal: bounds-checked map lookup, not declared in stdlib.
    pub const MAP_GET_CHECKED: &str = "miri_rt_map_get_checked";
    /// Compiler-internal: registers the value drop function, not in stdlib.
    pub const MAP_SET_VAL_DROP_FN: &str = "miri_rt_map_set_val_drop_fn";
    /// Compiler-internal: registers the key drop function, not in stdlib.
    pub const MAP_SET_KEY_DROP_FN: &str = "miri_rt_map_set_key_drop_fn";
    /// Compiler-internal: registers the value clone function, not in stdlib.
    pub const MAP_SET_VAL_CLONE_FN: &str = "miri_rt_map_set_val_clone_fn";
    /// Compiler-internal: selects how keys are hashed and compared, not in stdlib.
    pub const MAP_SET_KEY_KIND: &str = "miri_rt_map_set_key_kind";
    pub const MAP_CLONE: &str = "miri_rt_map_clone";
    /// Compiler-internal: Copy-on-Write check before mutation, not in stdlib.
    pub const MAP_COW: &str = "miri_rt_map_cow";
    /// Compiler-internal: decrements the RC of a map element, not in stdlib.
    pub const MAP_DECREF_ELEMENT: &str = "miri_rt_map_decref_element";

    // ── Set ──────────────────────────────────────────────────────────────────
    pub const SET_NEW: &str = "miri_rt_set_new";
    pub const SET_FREE: &str = "miri_rt_set_free";
    pub const SET_LEN: &str = "miri_rt_set_len";
    pub const SET_ADD: &str = "miri_rt_set_add";
    pub const SET_CONTAINS: &str = "miri_rt_set_contains";
    pub const SET_REMOVE: &str = "miri_rt_set_remove";
    pub const SET_CLEAR: &str = "miri_rt_set_clear";
    pub const SET_IS_EMPTY: &str = "miri_rt_set_is_empty";
    pub const SET_ELEMENT_AT: &str = "miri_rt_set_element_at";
    pub const SET_CLONE: &str = "miri_rt_set_clone";
    /// Compiler-internal: Copy-on-Write check before mutation, not in stdlib.
    pub const SET_COW: &str = "miri_rt_set_cow";
    /// Compiler-internal: registers the element drop function, not in stdlib.
    pub const SET_SET_ELEM_DROP_FN: &str = "miri_rt_set_set_elem_drop_fn";
    /// Compiler-internal: registers the element clone function, not in stdlib.
    pub const SET_SET_ELEM_CLONE_FN: &str = "miri_rt_set_set_elem_clone_fn";
    /// Compiler-internal: decrements the RC of a set element, not in stdlib.
    pub const SET_DECREF_ELEMENT: &str = "miri_rt_set_decref_element";

    // ── IO ───────────────────────────────────────────────────────────────────
    pub const PRINT: &str = "miri_rt_print";
    pub const PRINTLN: &str = "miri_rt_println";
    pub const EPRINT: &str = "miri_rt_eprint";
    pub const EPRINTLN: &str = "miri_rt_eprintln";
    pub const GET_LINE_END: &str = "miri_rt_get_line_end";
    pub const PANIC: &str = "miri_rt_panic";
    /// Compiler-internal: catch-and-validate a panic raised by a Miri closure.
    pub const ASSERT_PANICS: &str = "miri_rt_assert_panics";
    /// Compiler-internal: formats and aborts on a failed `assert(cond)`.
    pub const ASSERT_FAIL: &str = "miri_rt_assert_fail";
    /// Compiler-internal: formats and aborts on a failed `assert_eq(a, b)`.
    pub const ASSERT_EQ_FAIL: &str = "miri_rt_assert_eq_fail";
    /// Compiler-internal: formats and aborts on a failed `assert_ne(a, b)`.
    pub const ASSERT_NE_FAIL: &str = "miri_rt_assert_ne_fail";
    /// Compiler-internal: prints "division by zero" and `_exit(1)`s.
    /// Replaces the Cranelift `trapz` instruction so the process terminates
    /// cleanly without raising SIGTRAP/SIGILL (which on macOS would spawn the
    /// `ReportCrash` daemon and serialize parallel test runs).
    pub const DIV_BY_ZERO_PANIC: &str = "miri_rt_div_by_zero_panic";

    // ── String ────────────────────────────────────────────────────────────────
    pub const STRING_NEW: &str = "miri_rt_string_new";
    pub const STRING_FREE: &str = "miri_rt_string_free";
    /// Compiler-internal: RC-decrementing drop callback for string map keys.
    pub const STRING_DECREF_ELEMENT: &str = "miri_rt_string_decref_element";
    pub const STRING_LEN: &str = "miri_rt_string_len";
    pub const STRING_CHAR_COUNT: &str = "miri_rt_string_char_count";
    pub const STRING_IS_EMPTY: &str = "miri_rt_string_is_empty";
    pub const STRING_CONCAT: &str = "miri_rt_string_concat";
    pub const STRING_CLONE: &str = "miri_rt_string_clone";
    pub const STRING_EQUALS: &str = "miri_rt_string_equals";
    pub const STRING_CONTAINS: &str = "miri_rt_string_contains";
    pub const STRING_STARTS_WITH: &str = "miri_rt_string_starts_with";
    pub const STRING_ENDS_WITH: &str = "miri_rt_string_ends_with";
    pub const STRING_TO_LOWER: &str = "miri_rt_string_to_lower";
    pub const STRING_TO_UPPER: &str = "miri_rt_string_to_upper";
    pub const STRING_TRIM: &str = "miri_rt_string_trim";
    pub const STRING_TRIM_START: &str = "miri_rt_string_trim_start";
    pub const STRING_TRIM_END: &str = "miri_rt_string_trim_end";
    pub const STRING_REPLACE: &str = "miri_rt_string_replace";
    pub const STRING_SUBSTRING: &str = "miri_rt_string_substring";
    pub const STRING_REPEAT: &str = "miri_rt_string_repeat";
    pub const STRING_CHAR_AT: &str = "miri_rt_string_char_at";
    pub const STRING_SPLIT: &str = "miri_rt_string_split";
    pub const STRING_JOIN: &str = "miri_rt_string_join";
    pub const STRING_PARSE_INT: &str = "miri_rt_string_parse_int";
    pub const STRING_PARSE_INT_STATUS: &str = "miri_rt_string_parse_int_status";
    pub const STRING_PARSE_FLOAT: &str = "miri_rt_string_parse_float";
    pub const STRING_PARSE_FLOAT_STATUS: &str = "miri_rt_string_parse_float_status";
    pub const STRING_PARSE_STATUS: &str = "miri_rt_string_parse_status";
    pub const STRING_NEXT_CHAR_BOUNDARY: &str = "miri_rt_string_next_char_boundary";
    pub const STRING_CODE_AT: &str = "miri_rt_string_code_at";
    pub const STRING_FROM_CODE_POINT: &str = "miri_rt_string_from_code_point";

    // ── String conversion ────────────────────────────────────────────────────
    /// Compiler-internal: used by the codegen for int → String coercions.
    pub const BOOL_TO_STRING: &str = "miri_rt_bool_to_string";
    /// Compiler-internal: used by the codegen for f64 → String coercions.
    pub const FLOAT_TO_STRING: &str = "miri_rt_float_to_string";
    /// Compiler-internal: used by the codegen for f32 → String coercions, so an
    /// f32 formats with its own shortest representation instead of the extra
    /// digits an f64 promotion would expose.
    pub const F32_TO_STRING: &str = "miri_rt_f32_to_string";
    /// Compiler-internal: used by the codegen for signed int → String coercions.
    pub const INT_TO_STRING: &str = "miri_rt_int_to_string";
    /// Compiler-internal: used by the codegen for unsigned int → String
    /// coercions, so a value >= 2^63 formats as its unsigned magnitude rather
    /// than a negative `i64`.
    pub const UINT_TO_STRING: &str = "miri_rt_uint_to_string";

    // ── Filesystem ────────────────────────────────────────────────────────────
    pub const FS_STATUS: &str = "miri_rt_fs_status";
    pub const FS_ERROR_MESSAGE: &str = "miri_rt_fs_error_message";
    pub const FS_EXISTS: &str = "miri_rt_fs_exists";
    pub const FS_READ_FILE: &str = "miri_rt_fs_read_file";
    pub const FS_WRITE_FILE: &str = "miri_rt_fs_write_file";
    pub const FS_APPEND_FILE: &str = "miri_rt_fs_append_file";
    pub const FS_LIST_DIR: &str = "miri_rt_fs_list_dir";
    pub const FS_CREATE_DIR: &str = "miri_rt_fs_create_dir";
    pub const FS_DELETE: &str = "miri_rt_fs_delete";
    pub const FS_CWD: &str = "miri_rt_fs_cwd";

    // ── OS / Environment ──────────────────────────────────────────────────────
    pub const ENV_HAS: &str = "miri_rt_env_has";
    pub const ENV_GET: &str = "miri_rt_env_get";
    pub const ENV_SET: &str = "miri_rt_env_set";
    pub const ENV_STATUS: &str = "miri_rt_env_status";
    pub const ENV_ERROR_MESSAGE: &str = "miri_rt_env_error_message";
    pub const ARGS_COUNT: &str = "miri_rt_args_count";
    pub const ARGS_AT: &str = "miri_rt_args_at";
    pub const PLATFORM: &str = "miri_rt_platform";

    // ── Process ────────────────────────────────────────────────────────────────
    pub const EXIT: &str = "miri_rt_exit";

    // ── Time ─────────────────────────────────────────────────────────────────
    pub const NANOTIME: &str = "miri_rt_nanotime";
    pub const SLEEP_NANOS: &str = "miri_rt_sleep_nanos";

    // ── Regex ─────────────────────────────────────────────────────────────────
    pub const REGEX_COMPILE: &str = "miri_rt_regex_compile";
    pub const REGEX_COMPILE_STATUS: &str = "miri_rt_regex_compile_status";
    pub const REGEX_COMPILE_MESSAGE: &str = "miri_rt_regex_compile_message";
    pub const REGEX_FROM_VALIDATED_PATTERN: &str = "miri_rt_regex_from_validated_pattern";
    pub const REGEX_MATCHES: &str = "miri_rt_regex_matches";
    pub const REGEX_FIND: &str = "miri_rt_regex_find";
    pub const REGEX_FIND_FROM: &str = "miri_rt_regex_find_from";
    pub const REGEX_MATCH_START: &str = "miri_rt_regex_match_start";
    pub const REGEX_MATCH_END: &str = "miri_rt_regex_match_end";
    pub const REGEX_REPLACE: &str = "miri_rt_regex_replace";

    // ── Complete symbol table ────────────────────────────────────────────────
    //
    // Every constant above must appear here.  The drift-check tests in
    // `tests/stdlib/runtime_fns_sync.rs` use this slice to verify:
    //   (a) every `runtime "core" fn` in a stdlib `.mi` file has an entry, and
    //   (b) every entry is exported from the compiled runtime library.
    pub const ALL: &[&str] = &[
        // Closure (compiler-internal + test-only)
        CLOSURE_ALLOC_TRACK,
        CLOSURE_FREE_TRACK,
        CLOSURE_SIMULATE_LEAK,
        SIMULATE_DOUBLE_FREE,
        CLASS_ALLOC_TRACK,
        CLASS_FREE_TRACK,
        TRACKING_STATE,
        // Array
        ARRAY_NEW,
        ARRAY_FREE,
        ARRAY_LEN,
        ARRAY_SET_VAL,
        ARRAY_SORT,
        ARRAY_CLONE,
        ARRAY_SLICE,
        ARRAY_PANIC_OOB,
        ARRAY_DECREF_ELEMENT,
        ARRAY_SET_ELEM_DROP_FN,
        ARRAY_SET_ELEM_CLONE_FN,
        // Tuple
        TUPLE_LEN,
        // List
        LIST_NEW,
        LIST_FREE,
        LIST_LEN,
        LIST_PUSH,
        LIST_POP,
        LIST_SET,
        LIST_INSERT,
        LIST_REMOVE,
        LIST_TAKE_AT,
        LIST_CLEAR,
        LIST_REVERSE,
        LIST_SORT,
        LIST_IS_EMPTY,
        LIST_CLONE,
        LIST_COW,
        LIST_NEW_FROM_RAW,
        LIST_NEW_FROM_MANAGED_ARRAY,
        LIST_DECREF_ELEMENT,
        LIST_SET_ELEM_DROP_FN,
        LIST_SET_ELEM_CLONE_FN,
        // Map
        MAP_NEW,
        MAP_FREE,
        MAP_LEN,
        MAP_IS_EMPTY,
        MAP_SET,
        MAP_GET,
        MAP_CONTAINS_KEY,
        MAP_REMOVE,
        MAP_CLEAR,
        MAP_KEY_AT,
        MAP_VALUE_AT,
        MAP_CLONE,
        MAP_COW,
        MAP_GET_CHECKED,
        MAP_SET_VAL_DROP_FN,
        MAP_SET_KEY_DROP_FN,
        MAP_SET_VAL_CLONE_FN,
        MAP_SET_KEY_KIND,
        MAP_DECREF_ELEMENT,
        // Set
        SET_NEW,
        SET_FREE,
        SET_LEN,
        SET_ADD,
        SET_CONTAINS,
        SET_REMOVE,
        SET_CLEAR,
        SET_IS_EMPTY,
        SET_ELEMENT_AT,
        SET_CLONE,
        SET_COW,
        SET_SET_ELEM_DROP_FN,
        SET_SET_ELEM_CLONE_FN,
        SET_DECREF_ELEMENT,
        // IO
        PRINT,
        PRINTLN,
        EPRINT,
        EPRINTLN,
        GET_LINE_END,
        PANIC,
        ASSERT_PANICS,
        ASSERT_FAIL,
        ASSERT_EQ_FAIL,
        ASSERT_NE_FAIL,
        DIV_BY_ZERO_PANIC,
        // Filesystem
        FS_STATUS,
        FS_ERROR_MESSAGE,
        FS_EXISTS,
        FS_READ_FILE,
        FS_WRITE_FILE,
        FS_APPEND_FILE,
        FS_LIST_DIR,
        FS_CREATE_DIR,
        FS_DELETE,
        FS_CWD,
        // OS / Environment
        ENV_HAS,
        ENV_GET,
        ENV_SET,
        ENV_STATUS,
        ENV_ERROR_MESSAGE,
        ARGS_COUNT,
        ARGS_AT,
        PLATFORM,
        // Process
        EXIT,
        // String
        STRING_NEW,
        STRING_FREE,
        STRING_DECREF_ELEMENT,
        STRING_LEN,
        STRING_CHAR_COUNT,
        STRING_IS_EMPTY,
        STRING_CONCAT,
        STRING_CLONE,
        STRING_EQUALS,
        STRING_CONTAINS,
        STRING_STARTS_WITH,
        STRING_ENDS_WITH,
        STRING_TO_LOWER,
        STRING_TO_UPPER,
        STRING_TRIM,
        STRING_TRIM_START,
        STRING_TRIM_END,
        STRING_REPLACE,
        STRING_SUBSTRING,
        STRING_REPEAT,
        STRING_CHAR_AT,
        STRING_SPLIT,
        STRING_JOIN,
        STRING_PARSE_INT,
        STRING_PARSE_INT_STATUS,
        STRING_PARSE_FLOAT,
        STRING_PARSE_FLOAT_STATUS,
        STRING_PARSE_STATUS,
        STRING_NEXT_CHAR_BOUNDARY,
        STRING_CODE_AT,
        STRING_FROM_CODE_POINT,
        // String conversion (compiler-internal)
        BOOL_TO_STRING,
        FLOAT_TO_STRING,
        F32_TO_STRING,
        INT_TO_STRING,
        UINT_TO_STRING,
        // Time
        NANOTIME,
        SLEEP_NANOS,
        // Regex
        REGEX_COMPILE,
        REGEX_COMPILE_STATUS,
        REGEX_COMPILE_MESSAGE,
        REGEX_FROM_VALIDATED_PATTERN,
        REGEX_MATCHES,
        REGEX_FIND,
        REGEX_FIND_FROM,
        REGEX_MATCH_START,
        REGEX_MATCH_END,
        REGEX_REPLACE,
    ];

    /// Value of [`TRACKING_STATE`] meaning the runtime wants no report, so
    /// compiled code may skip the hook. Any other value — including the initial
    /// unset one, which is why the first allocation always calls in — means
    /// report it. A value rather than a symbol name, so it belongs below the
    /// table above rather than in it.
    pub const TRACKING_STATE_OFF: i64 = 1;
}

use crate::ast::types::BuiltinCollectionKind;

/// Argument positions whose reference `name` takes ownership of, in call order.
///
/// Two shapes reach the same place. A container intrinsic that stores what it is
/// handed keeps that reference: the element is released later by the container's
/// drop callback, never by the code that built it, and lowering pays for this by
/// retaining the value into the temp it passes. A copy-on-write entry point takes
/// its receiver over instead: it hands back either that same value or a fresh
/// clone, releasing the original in the clone case, so exactly one reference goes
/// in and one comes out.
///
/// Either way the caller stops owning what it passed, which is what anything
/// reasoning about reference counts over MIR needs to know. The receiver of a
/// storing intrinsic is not listed — those mutate in place, so the caller keeps
/// holding the container after the call returns.
///
/// Empty for every other symbol: the readers borrow their arguments, and a call
/// that builds something new out of what it is given leaves the originals with the
/// caller to release.
pub fn taken_argument_positions(name: &str) -> &'static [usize] {
    match name {
        rt::LIST_PUSH | rt::SET_ADD => &[1],
        rt::LIST_INSERT => &[2],
        rt::MAP_SET => &[1, 2],
        rt::LIST_COW | rt::MAP_COW | rt::SET_COW => &[0],
        _ => &[],
    }
}

/// Argument positions of `name` that carry an element value as opaque bytes.
///
/// A collection stores whatever bit pattern it is handed and compares later
/// lookups against those same bytes, so an element argument is never a number to
/// the runtime — it is the element's representation widened into a value word.
/// Converting it numerically would store one thing and search for another: a
/// float turned into the integer nearest its value can never match the float
/// that was stored.
///
/// Both the storing entry points and the lookup ones are listed, because a
/// lookup that reinterprets differently from the store it must match is exactly
/// the mismatch this prevents. Positions naming an index or a size are not
/// listed; those really are numbers.
pub fn element_value_positions(name: &str) -> &'static [usize] {
    match name {
        rt::LIST_PUSH | rt::SET_ADD | rt::SET_CONTAINS | rt::SET_REMOVE => &[1],
        rt::MAP_GET | rt::MAP_CONTAINS_KEY | rt::MAP_REMOVE | rt::MAP_GET_CHECKED => &[1],
        rt::LIST_SET | rt::LIST_INSERT => &[2],
        rt::MAP_SET => &[1, 2],
        _ => &[],
    }
}

/// Whether `name` hands its caller an element value as opaque bytes.
///
/// A container stores element bytes in a value word and returns that same word,
/// whatever the element's declared type is. Declaring the call as returning a
/// float instead would read the result from the register floats are returned in
/// while the runtime wrote the one integers use, so the value would arrive as
/// zero. The word is reinterpreted at the destination instead.
pub fn returns_element_value(name: &str) -> bool {
    matches!(
        name,
        rt::MAP_GET | rt::MAP_GET_CHECKED | rt::MAP_KEY_AT | rt::MAP_VALUE_AT | rt::SET_ELEMENT_AT
    )
}

/// Whether `name` hands back a reference its container keeps owning.
///
/// Indexing a map reads through to the entry the map still holds — `m[k]` is
/// consumed in place by the expression around it, which never releases what it
/// read. Treating that as a fresh reference would report it as one nobody released.
///
/// Every other call hands back something its caller owns: a reader that raises the
/// count before returning, or a value built on the spot.
pub fn hands_back_a_borrow(name: &str) -> bool {
    name == rt::MAP_GET_CHECKED
}

/// Whether `name` never returns to its caller.
///
/// Each of these reports a failure and ends the process, or unwinds past the call
/// through the trap the testing harness installs. Control does not come back, so
/// the block the call names as its successor is not reached along that path and
/// nothing the caller was still holding there is ever released — correctly, since
/// there is no one left to release it for.
pub fn diverges(name: &str) -> bool {
    matches!(
        name,
        rt::PANIC
            | rt::ASSERT_FAIL
            | rt::ASSERT_EQ_FAIL
            | rt::ASSERT_NE_FAIL
            | rt::ARRAY_PANIC_OOB
            | rt::DIV_BY_ZERO_PANIC
    )
}

/// Returns the Copy-on-Write runtime function for a built-in collection kind,
/// or `None` for kinds that do not have a CoW intrinsic (`Array`).
///
/// Centralized so dispatch logic does not branch on string class names.
pub fn cow_fn(kind: BuiltinCollectionKind) -> Option<&'static str> {
    match kind {
        BuiltinCollectionKind::List => Some(rt::LIST_COW),
        BuiltinCollectionKind::Set => Some(rt::SET_COW),
        BuiltinCollectionKind::Map => Some(rt::MAP_COW),
        BuiltinCollectionKind::Array => None,
    }
}
