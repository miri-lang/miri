//! Tuple support for Miri runtime.
//!
//! Tuples are heap-allocated by the Cranelift backend with the layout:
//! `[malloc_ptr][RC][elem_count][field0][field1]...`
//!
//! The pointer stored in a Miri variable points to the payload, i.e. past the
//! `[malloc_ptr][RC]` header, so `elem_count` is at offset 0.

// =============================================================================
// FFI Functions
// =============================================================================

/// Returns the number of elements in a tuple.
///
/// The pointer must be a valid Miri tuple payload pointer (past the
/// `[malloc_ptr][RC]` header). Returns 0 for null pointers.
#[no_mangle]
#[allow(clippy::missing_safety_doc)]
pub unsafe extern "C" fn miri_rt_tuple_len(ptr: *const usize) -> usize {
    if ptr.is_null() {
        return 0;
    }
    *ptr
}
