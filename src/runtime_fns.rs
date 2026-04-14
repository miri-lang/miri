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
    // ── Array ────────────────────────────────────────────────────────────────
    pub const ARRAY_NEW: &str = "miri_rt_array_new";
    pub const ARRAY_FREE: &str = "miri_rt_array_free";
    pub const ARRAY_LEN: &str = "miri_rt_array_len";
    pub const ARRAY_SET_VAL: &str = "miri_rt_array_set_val";
    pub const ARRAY_SORT: &str = "miri_rt_array_sort";
    /// Compiler-internal: bounds-check panic helper, not declared in stdlib.
    pub const ARRAY_PANIC_OOB: &str = "miri_rt_array_panic_oob";
    /// Compiler-internal: registers the element drop function, not in stdlib.
    pub const ARRAY_SET_ELEM_DROP_FN: &str = "miri_rt_array_set_elem_drop_fn";

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
    pub const LIST_CLEAR: &str = "miri_rt_list_clear";
    pub const LIST_REVERSE: &str = "miri_rt_list_reverse";
    pub const LIST_SORT: &str = "miri_rt_list_sort";
    pub const LIST_IS_EMPTY: &str = "miri_rt_list_is_empty";
    /// Compiler-internal: constructs a list from a raw pointer, not in stdlib.
    pub const LIST_NEW_FROM_RAW: &str = "miri_rt_list_new_from_raw";
    /// Compiler-internal: constructs a list from a managed array, not in stdlib.
    pub const LIST_NEW_FROM_MANAGED_ARRAY: &str = "miri_rt_list_new_from_managed_array";
    /// Compiler-internal: decrements the RC of a list element, not in stdlib.
    pub const LIST_DECREF_ELEMENT: &str = "miri_rt_list_decref_element";
    /// Compiler-internal: registers the element drop function, not in stdlib.
    pub const LIST_SET_ELEM_DROP_FN: &str = "miri_rt_list_set_elem_drop_fn";

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

    // ── IO ───────────────────────────────────────────────────────────────────
    pub const PRINT: &str = "miri_rt_print";
    pub const PRINTLN: &str = "miri_rt_println";
    pub const EPRINT: &str = "miri_rt_eprint";
    pub const EPRINTLN: &str = "miri_rt_eprintln";
    pub const GET_LINE_END: &str = "miri_rt_get_line_end";

    // ── String ────────────────────────────────────────────────────────────────
    pub const STRING_NEW: &str = "miri_rt_string_new";
    pub const STRING_FREE: &str = "miri_rt_string_free";
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

    // ── String conversion ────────────────────────────────────────────────────
    /// Compiler-internal: used by the codegen for int → String coercions.
    pub const BOOL_TO_STRING: &str = "miri_rt_bool_to_string";
    /// Compiler-internal: used by the codegen for float → String coercions.
    pub const FLOAT_TO_STRING: &str = "miri_rt_float_to_string";
    /// Compiler-internal: used by the codegen for int → String coercions.
    pub const INT_TO_STRING: &str = "miri_rt_int_to_string";

    // ── Time ─────────────────────────────────────────────────────────────────
    pub const NANOTIME: &str = "miri_rt_nanotime";

    // ── Complete symbol table ────────────────────────────────────────────────
    //
    // Every constant above must appear here.  The drift-check tests in
    // `tests/stdlib/runtime_fns_sync.rs` use this slice to verify:
    //   (a) every `runtime "core" fn` in a stdlib `.mi` file has an entry, and
    //   (b) every entry is exported from the compiled runtime library.
    pub const ALL: &[&str] = &[
        // Array
        ARRAY_NEW,
        ARRAY_FREE,
        ARRAY_LEN,
        ARRAY_SET_VAL,
        ARRAY_SORT,
        ARRAY_PANIC_OOB,
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
        LIST_CLEAR,
        LIST_REVERSE,
        LIST_SORT,
        LIST_IS_EMPTY,
        LIST_NEW_FROM_RAW,
        LIST_NEW_FROM_MANAGED_ARRAY,
        LIST_DECREF_ELEMENT,
        LIST_SET_ELEM_DROP_FN,
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
        MAP_GET_CHECKED,
        MAP_SET_VAL_DROP_FN,
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
        // IO
        PRINT,
        PRINTLN,
        EPRINT,
        EPRINTLN,
        GET_LINE_END,
        // String
        STRING_NEW,
        STRING_FREE,
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
        // String conversion (compiler-internal)
        BOOL_TO_STRING,
        FLOAT_TO_STRING,
        INT_TO_STRING,
        // Time
        NANOTIME,
    ];
}
