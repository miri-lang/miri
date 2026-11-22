// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The check that a block whose release runs a drop hook has one holder.
//!
//! A class or enum with a `fn drop(self)` hook is a resource: one owner, moved
//! on assignment, its hook run exactly once. Its block's reference count is
//! therefore never more than one. A count raised past one means two places
//! hold the resource, and the hook would run when the second lets go rather
//! than when the first does — a compiler defect that no output shows.
//!
//! Under `MIRI_HOOK_CHECK=1`, in a build without `--release`, compiled code
//! marks each such block right after building it, and every increment —
//! compiled code's and the runtime's own — asks here whether it raised a
//! marked block past one. A program with no marked block pays one atomic load
//! per increment.
//!
//! The check is asked for rather than on under every leak check because the
//! compiler does not keep the rule yet: it still hands a resource to a
//! collection, a field, a constructed value or a reassigned binding by
//! retaining it rather than moving it, and reads an element or field of a
//! resource through a retained temporary.

use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use crate::guard::INLINE_PAYLOAD_OFFSET;
use crate::rc::RC_HEADER_SIZE;

/// The payload addresses of the live blocks whose release runs a drop hook.
static HOOKED: Mutex<Option<HashSet<usize>>> = Mutex::new(None);

/// How many blocks [`HOOKED`] holds, read without its lock so a program with
/// none skips every lookup.
static HOOKED_LIVE: AtomicUsize = AtomicUsize::new(0);

/// Who made an increment the check looks at, named in its report.
#[derive(Clone, Copy)]
pub enum Increment {
    /// An increment compiled code emitted.
    CompiledCode,
    /// An increment the runtime made, at the intrinsic that made it.
    Runtime(&'static std::panic::Location<'static>),
}

impl std::fmt::Display for Increment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Increment::CompiledCode => f.write_str("compiled code"),
            Increment::Runtime(at) => write!(f, "the runtime at {at}"),
        }
    }
}

/// Runs `f` on the set of hooked blocks, recovering it when an earlier
/// panicking holder poisoned the lock.
fn with_hooked<R>(f: impl FnOnce(&mut HashSet<usize>) -> R) -> R {
    let mut guard = HOOKED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(guard.get_or_insert_with(HashSet::new))
}

/// Marks the block at `payload` as one whose release runs a drop hook, when
/// `MIRI_HOOK_CHECK=1` asked for the check.
///
/// Codegen calls this, while the runtime tracks, right after it builds a
/// class or enum instance whose type has a hook. A null `payload` is ignored.
///
/// # Safety
/// `payload` is null or the payload pointer of a live block.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_hooked_block_mark(payload: *mut u8) {
    if crate::handler_config::is_hook_check_enabled() {
        mark(payload);
    }
}

/// Records the block at `payload` as hooked. A null `payload` is ignored.
fn mark(payload: *mut u8) {
    if payload.is_null() {
        return;
    }
    if with_hooked(|hooked| hooked.insert(payload as usize)) {
        HOOKED_LIVE.fetch_add(1, Ordering::Relaxed);
    }
}

/// Forgets the block allocated at `base`, which compiled code is about to
/// release, so a later block at the same address starts unmarked.
pub(crate) fn forget_block(base: *mut u8) {
    if base.is_null() || HOOKED_LIVE.load(Ordering::Relaxed) == 0 {
        return;
    }
    let payload = (base as usize).wrapping_add(INLINE_PAYLOAD_OFFSET);
    if with_hooked(|hooked| hooked.remove(&payload)) {
        HOOKED_LIVE.fetch_sub(1, Ordering::Relaxed);
    }
}

/// Stops the program when the increment just made to the block at `payload`
/// raised a hooked block's count past one.
///
/// Codegen calls this, while the runtime tracks, after each increment it
/// emits; the runtime's own increments ask through [`check_retain`].
///
/// # Safety
/// `payload` is null or the payload pointer of a live counted block.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_retain_check(payload: *mut u8) {
    check_retain(payload, Increment::CompiledCode);
}

/// The check behind [`miri_rt_retain_check`], for the runtime's own
/// increments. `by` names who made the increment, for the report.
///
/// # Safety
/// `payload` is null or the payload pointer of a live counted block.
pub(crate) unsafe fn check_retain(payload: *mut u8, by: Increment) {
    if payload.is_null() || HOOKED_LIVE.load(Ordering::Relaxed) == 0 {
        return;
    }
    if !with_hooked(|hooked| hooked.contains(&(payload as usize))) {
        return;
    }
    let count = payload.sub(RC_HEADER_SIZE).cast::<usize>().read();
    if crate::abi_layout::is_immortal(count) || count <= 1 {
        return;
    }
    crate::trap::trap(
        crate::trap::code::HOOKED_BLOCK_SHARED,
        &format!(
            "a value whose release runs a drop hook gained a second holder \
             (count {count}, raised by {by}), so its hook would run when the last \
             holder lets go rather than when its owner does; this is a compiler defect"
        ),
    )
}

/// Builds one hooked block and raises its count to two, for testing that the
/// check stops the program with `MER_RT_021`, whether or not the check was
/// asked for. The block is never released.
///
/// # Safety
/// This function is for testing only; the increment stops the program.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_test_simulate_shared_hooked_block() {
    let base = crate::block_cache::ffi::miri_rt_block_alloc(INLINE_PAYLOAD_OFFSET);
    if base.is_null() {
        return;
    }
    let payload = base.add(INLINE_PAYLOAD_OFFSET);
    payload.sub(RC_HEADER_SIZE).cast::<usize>().write(1);
    mark(payload);
    crate::rc::incref(payload);
}
