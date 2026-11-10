// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_core::rc::*;

/// Clones `original` once a second holder has made it shared, and checks the
/// clone is held by nothing else.
unsafe fn assert_clone_is_unshared<T>(
    kind: &str,
    original: *mut T,
    clone: unsafe extern "C" fn(*const T) -> *mut T,
    free: unsafe extern "C" fn(*mut T),
) {
    incref(original as *mut u8);
    assert!(
        miri_rt_rc_is_shared(original as *const u8),
        "{kind} original"
    );
    let copy = clone(original);
    assert!(!miri_rt_rc_is_shared(copy as *const u8), "{kind} clone");
    free(original);
    free(copy);
}

/// The compiler takes a value a collection clone hands back as held by
/// nothing else, and skips asking whether it is shared before writing
/// through it (`returns_unshared` in the compiler's runtime function table).
/// Each clone must therefore return a fresh block whose count is 1, even when
/// the collection cloned is shared.
#[test]
fn collection_clones_hand_back_blocks_nothing_else_holds() {
    use miri_runtime_core::array::ffi::{
        miri_rt_array_clone, miri_rt_array_free, miri_rt_array_new,
    };
    use miri_runtime_core::list::ffi::{miri_rt_list_clone, miri_rt_list_free, miri_rt_list_new};
    use miri_runtime_core::map::ffi::{miri_rt_map_clone, miri_rt_map_free, miri_rt_map_new};
    use miri_runtime_core::set::ffi::{miri_rt_set_clone, miri_rt_set_free, miri_rt_set_new};
    unsafe {
        let list = miri_rt_list_new(8);
        assert_clone_is_unshared("list", list, miri_rt_list_clone, miri_rt_list_free);
        let map = miri_rt_map_new(8, 8, 0);
        assert_clone_is_unshared("map", map, miri_rt_map_clone, miri_rt_map_free);
        let set = miri_rt_set_new(8);
        assert_clone_is_unshared("set", set, miri_rt_set_clone, miri_rt_set_free);
        let array = miri_rt_array_new(3, 8);
        assert_clone_is_unshared("array", array, miri_rt_array_clone, miri_rt_array_free);
    }
}

/// A writing method checks its receiver arrived held by its call alone. The
/// check only reads: a receiver that passes it keeps its count, so the method
/// owns no reference more or less than it was handed.
#[test]
fn a_receiver_held_once_passes_the_uniqueness_check_untouched() {
    use miri_runtime_core::list::ffi::{miri_rt_list_free, miri_rt_list_new};
    unsafe {
        let list = miri_rt_list_new(8);
        miri_rt_receiver_unique(list as *const u8);
        assert!(!miri_rt_rc_is_shared(list as *const u8));
        miri_rt_receiver_unique(std::ptr::null());
        miri_rt_list_free(list);
    }
}
