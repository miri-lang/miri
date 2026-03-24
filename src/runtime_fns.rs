// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Runtime function name constants.
//!
//! Centralizes every `miri_rt_*` symbol name so that renaming a runtime
//! function requires changing exactly one place in the compiler rather than
//! hunting down scattered string literals.

/// Constants for all `miri_rt_*` runtime symbols.
pub mod rt {
    // ── Array ────────────────────────────────────────────────────────────────
    pub const ARRAY_NEW: &str = "miri_rt_array_new";
    pub const ARRAY_FREE: &str = "miri_rt_array_free";
    pub const ARRAY_LEN: &str = "miri_rt_array_len";
    pub const ARRAY_SET_VAL: &str = "miri_rt_array_set_val";
    pub const ARRAY_PANIC_OOB: &str = "miri_rt_array_panic_oob";

    // ── Tuple ─────────────────────────────────────────────────────────────────
    pub const TUPLE_LEN: &str = "miri_rt_tuple_len";

    // ── List ─────────────────────────────────────────────────────────────────
    pub const LIST_NEW: &str = "miri_rt_list_new";
    pub const LIST_NEW_FROM_RAW: &str = "miri_rt_list_new_from_raw";
    pub const LIST_NEW_FROM_MANAGED_ARRAY: &str = "miri_rt_list_new_from_managed_array";
    pub const LIST_PUSH: &str = "miri_rt_list_push";
    pub const LIST_INSERT: &str = "miri_rt_list_insert";
    pub const LIST_FREE: &str = "miri_rt_list_free";
    pub const LIST_DECREF_ELEMENT: &str = "miri_rt_list_decref_element";
    pub const LIST_SET_ELEM_DROP_FN: &str = "miri_rt_list_set_elem_drop_fn";

    // ── Map ──────────────────────────────────────────────────────────────────
    pub const MAP_NEW: &str = "miri_rt_map_new";
    pub const MAP_SET: &str = "miri_rt_map_set";
    pub const MAP_FREE: &str = "miri_rt_map_free";
    pub const MAP_GET_CHECKED: &str = "miri_rt_map_get_checked";
    pub const MAP_CONTAINS_KEY: &str = "miri_rt_map_contains_key";
    pub const MAP_SET_VAL_DROP_FN: &str = "miri_rt_map_set_val_drop_fn";

    // ── Set ──────────────────────────────────────────────────────────────────
    pub const SET_NEW: &str = "miri_rt_set_new";
    pub const SET_ADD: &str = "miri_rt_set_add";
    pub const SET_FREE: &str = "miri_rt_set_free";
    pub const SET_CONTAINS: &str = "miri_rt_set_contains";

    // ── String conversion ────────────────────────────────────────────────────
    pub const BOOL_TO_STRING: &str = "miri_rt_bool_to_string";
    pub const FLOAT_TO_STRING: &str = "miri_rt_float_to_string";
    pub const INT_TO_STRING: &str = "miri_rt_int_to_string";
}
