// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A per-thread cache of released small heap blocks, in front of the system
//! allocator.
//!
//! Reference-counted values are allocated and released at a very high rate,
//! and most are small: a class instance, a string's or a collection's header.
//! A released block of up to [`LARGEST_CACHED_BLOCK`] bytes is parked on the
//! releasing thread's list for its size class and handed out again, uncleared,
//! to a later request of that class. A block's size is rounded up to its class
//! when it is first allocated, so any parked block fits every request of its
//! class.
//!
//! A parked block's first word links it to the next one of its class and its
//! second word holds [`PARKED_SENTINEL`], a value no live block keeps there. Parking a
//! block that already carries it is a second release of the same block; taking
//! one that no longer does means it was written after it was released. Both end
//! the program with `MER_RT_015`, in every build.
//!
//! What the cache does not change:
//! - The leak check, `MIRI_ALLOC_COUNT` and `MIRI_RC_STATS` count logical
//!   allocations and releases, made before a block reaches or leaves the cache:
//!   a parked block was released, and a leaked block is never parked.
//! - The heap guard needs to see every block go back to the system — it
//!   quarantines released blocks to catch late uses — so while it is on the
//!   cache is bypassed and every block keeps its exact size.
//!
//! While a check or counter is asked for, or `MallocScribble` is set, and the
//! program was not built with `--release`, the cache poisons what it holds: see
//! [`CacheMode::Poisoned`].
//!
//! A thread's parked blocks are returned to the system when the thread exits;
//! a block released on a thread other than its allocating one is parked there.

use std::alloc::{alloc, dealloc, Layout};
use std::cell::Cell;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::trap::{code, trap};

/// Size classes are this many bytes apart.
const CLASS_GRANULE: usize = 16;

/// How many size classes are cached.
const CLASS_COUNT: usize = 16;

/// The largest block, in bytes, the cache parks.
pub const LARGEST_CACHED_BLOCK: usize = CLASS_GRANULE * CLASS_COUNT;

/// The most bytes one thread keeps parked; a release past it goes back to the
/// system, so a program that once held many values does not keep their memory.
/// The limit is per thread: a process can hold up to its thread count times
/// this much in parked blocks.
const PARKED_BYTES_LIMIT: usize = 16 << 20;

/// Alignment of every block. Inline blocks used to come straight from `malloc`,
/// which aligns to 16 on every 64-bit target the runtime supports, and their
/// payload starts two words in, so a 16-byte value laid out in one stays
/// aligned. Every class size is a multiple of it.
pub const BLOCK_ALIGN: usize = 16;

/// Bytes at the start of a parked block holding its link and its sentinel.
const PARKED_HEADER: usize = 2 * std::mem::size_of::<usize>();

/// How many released blocks of a class a poisoned cache holds back before it
/// hands the oldest out again, so a late use of a block meets its poison.
const QUARANTINE_DEPTH: usize = 64;

/// The byte a poisoned cache fills a released block with.
const RELEASED_FILL: u8 = 0xDD;

/// The byte a poisoned cache fills a block with before handing it out.
const HANDED_OUT_FILL: u8 = 0xAA;

/// How the process allocates small blocks, decided once on the first one.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[repr(u8)]
enum CacheMode {
    /// Not decided yet.
    Unset = 0,
    /// The heap guard is on: every block goes to and from the system at its
    /// exact size.
    Bypassed = 1,
    /// Released blocks are parked and the most recent is handed out first.
    Cached = 2,
    /// As [`CacheMode::Cached`], but each released block is filled with
    /// `0xDD` and held back behind [`QUARANTINE_DEPTH`] others, the fill is
    /// checked when the block is handed out again (a difference is a write
    /// after release), every handed-out block is filled with `0xAA`, and a
    /// block released at a size larger than the one it was allocated at is
    /// refused. A late read of a released block reads the fill rather than a
    /// new owner's data, as `MallocScribble` arranges for the system allocator.
    Poisoned = 3,
}

static CACHE_MODE: AtomicU8 = AtomicU8::new(CacheMode::Unset as u8);

/// The value the second word of a parked block holds.
///
/// No live block keeps it there: an inline block's second word is its
/// reference count, and a runtime header's is a data pointer or null, and no
/// pointer or count reaches bit 62. It is positive as a signed word, so compiled
/// code releasing a parked block once more treats it as a reference count and
/// writes to it, which taking the block then notices. A fixed value, rather
/// than one drawn per process, needs no ordering between threads: every thread
/// marks and checks with the same constant from its first block.
const PARKED_SENTINEL: usize = 0x4D49_5249_5041_524B;

/// A parked block's first word links it to the next one of its class.
struct ParkedBlock {
    next: *mut ParkedBlock,
}

/// What a poisoned cache keeps per size class to use its list as a queue,
/// oldest at the head: an unpoisoned cache uses the list as a stack through
/// its head alone, the cheapest shape for the fast path.
struct ClassQueue {
    tail: Cell<*mut ParkedBlock>,
    count: Cell<usize>,
}

/// Whether a thread's cache has arranged to be emptied when the thread exits.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Cleanup {
    Unregistered,
    Registered,
    Done,
}

/// One thread's parked blocks, one list per size class.
///
/// It has no destructor, so reaching it costs no check of whether one was
/// registered; [`CacheCleanup`] empties it when the thread exits.
struct BlockCache {
    heads: [Cell<*mut ParkedBlock>; CLASS_COUNT],
    queues: [ClassQueue; CLASS_COUNT],
    parked_bytes: Cell<usize>,
    cleanup: Cell<Cleanup>,
}

/// Empties the thread's [`BlockCache`] when the thread exits.
struct CacheCleanup;

thread_local! {
    static CACHE: BlockCache = const { BlockCache::new() };
    static CLEANUP: CacheCleanup = const { CacheCleanup };
}

impl ClassQueue {
    const fn new() -> Self {
        Self {
            tail: Cell::new(std::ptr::null_mut()),
            count: Cell::new(0),
        }
    }
}

impl BlockCache {
    const fn new() -> Self {
        Self {
            heads: [const { Cell::new(std::ptr::null_mut()) }; CLASS_COUNT],
            queues: [const { ClassQueue::new() }; CLASS_COUNT],
            parked_bytes: Cell::new(0),
            cleanup: Cell::new(Cleanup::Unregistered),
        }
    }

    /// Pushes `block` on the list of `class`, where the next pop finds it.
    #[inline(always)]
    fn push(&self, class: usize, block: *mut ParkedBlock) {
        // SAFETY: `block` is a released block this list now owns alone.
        unsafe {
            block.write(ParkedBlock {
                next: self.heads[class].get(),
            })
        };
        self.heads[class].set(block);
    }

    /// Pops the block of `class` pushed last, if there is one.
    #[inline(always)]
    fn pop(&self, class: usize) -> Option<*mut ParkedBlock> {
        let head = self.heads[class].get();
        if head.is_null() {
            return None;
        }
        // SAFETY: a non-null head is a block this list owns; its first word is
        // the link a push wrote.
        self.heads[class].set(unsafe { (*head).next });
        Some(head)
    }

    /// Queues `block` behind every other one of `class`.
    fn enqueue(&self, class: usize, block: *mut ParkedBlock) {
        let queue = &self.queues[class];
        // SAFETY: `block` is a released block this list now owns alone.
        unsafe {
            block.write(ParkedBlock {
                next: std::ptr::null_mut(),
            })
        };
        match queue.tail.get() {
            // SAFETY: a non-null tail is a block this list owns.
            tail if !tail.is_null() => unsafe { (*tail).next = block },
            _ => self.heads[class].set(block),
        }
        queue.tail.set(block);
        queue.count.set(queue.count.get() + 1);
    }

    /// Dequeues the oldest block of `class` once more than `held_back` are
    /// queued.
    fn dequeue_past(&self, class: usize, held_back: usize) -> Option<*mut ParkedBlock> {
        let queue = &self.queues[class];
        if queue.count.get() <= held_back {
            return None;
        }
        let head = self.pop(class)?;
        if self.heads[class].get().is_null() {
            queue.tail.set(std::ptr::null_mut());
        }
        queue.count.set(queue.count.get() - 1);
        Some(head)
    }

    /// The parked block of `class` handed out next, if there is one.
    #[inline(always)]
    fn take(&self, class: usize, mode: CacheMode) -> Option<*mut u8> {
        let block = match mode {
            CacheMode::Poisoned => self.dequeue_past(class, QUARANTINE_DEPTH),
            CacheMode::Unset | CacheMode::Bypassed | CacheMode::Cached => self.pop(class),
        }?
        .cast::<u8>();
        self.parked_bytes
            .set(self.parked_bytes.get().saturating_sub(class_size(class)));
        // SAFETY: `block` was parked at `class_size(class)` bytes, at least
        // `PARKED_HEADER`, and is owned by no one else now.
        unsafe { claim_parked(block, class_size(class), mode) };
        Some(block)
    }

    /// Parks `base` on the list of `class`, or answers `false` when this thread
    /// keeps as many bytes as it may or is exiting.
    #[inline(always)]
    fn park(&self, base: *mut u8, class: usize, mode: CacheMode) -> bool {
        let parked = self.parked_bytes.get() + class_size(class);
        if parked > PARKED_BYTES_LIMIT || !self.is_cleanup_arranged() {
            return false;
        }
        let block = base.cast::<ParkedBlock>();
        match mode {
            CacheMode::Poisoned => self.enqueue(class, block),
            CacheMode::Unset | CacheMode::Bypassed | CacheMode::Cached => self.push(class, block),
        }
        // SAFETY: `base` is a released block of `class_size(class)` bytes; its
        // second word follows the link `push_*` wrote.
        unsafe { mark_parked(base, class_size(class), mode) };
        self.parked_bytes.set(parked);
        true
    }

    /// Whether the thread will empty this cache on exit, arranging it on the
    /// first park.
    #[inline(always)]
    fn is_cleanup_arranged(&self) -> bool {
        match self.cleanup.get() {
            Cleanup::Registered => true,
            Cleanup::Done => false,
            Cleanup::Unregistered => {
                let is_registered = CLEANUP.try_with(|_| ()).is_ok();
                if is_registered {
                    self.cleanup.set(Cleanup::Registered);
                }
                is_registered
            }
        }
    }

    /// Returns every parked block to the system.
    fn release_all(&self) {
        for (class, queue) in self.queues.iter().enumerate() {
            while let Some(block) = self.pop(class) {
                // SAFETY: every listed block was allocated at its class size.
                unsafe { release_to_system(block.cast(), class_size(class)) };
            }
            queue.tail.set(std::ptr::null_mut());
            queue.count.set(0);
        }
        self.parked_bytes.set(0);
    }
}

impl Drop for CacheCleanup {
    fn drop(&mut self) {
        CACHE.with(|cache| {
            cache.cleanup.set(Cleanup::Done);
            cache.release_all();
        });
    }
}

/// This thread's cache.
///
/// `CACHE` is initialised by a constant and has no destructor, so its storage
/// lives exactly as long as the thread and is never torn down: a reference to
/// it is valid for any call made on the thread. Taking the address once keeps
/// the work out of the closure `with` runs, which would otherwise be too large
/// to inline into every allocation.
#[inline(always)]
fn this_thread_cache() -> &'static BlockCache {
    let cache = CACHE.with(std::ptr::from_ref);
    // SAFETY: see above; the reference never leaves this thread, since
    // `BlockCache` is not `Sync`.
    unsafe { &*cache }
}

/// The bytes every block of `class` has.
const fn class_size(class: usize) -> usize {
    (class + 1) * CLASS_GRANULE
}

/// How the process allocates small blocks.
#[inline(always)]
fn cache_mode() -> CacheMode {
    match CACHE_MODE.load(Ordering::Relaxed) {
        2 => CacheMode::Cached,
        3 => CacheMode::Poisoned,
        1 => CacheMode::Bypassed,
        _ => settle_cache_mode(),
    }
}

/// Decides, once for the process, how small blocks are allocated.
#[cold]
fn settle_cache_mode() -> CacheMode {
    let mode = wanted_cache_mode();
    // Every thread settling at once reads the same environment, so a lost
    // race stores the same answer; the first stored one is what everyone uses.
    match CACHE_MODE.compare_exchange(
        CacheMode::Unset as u8,
        mode as u8,
        Ordering::Relaxed,
        Ordering::Relaxed,
    ) {
        Ok(_) => mode,
        Err(_) => cache_mode(),
    }
}

/// The mode the environment asks for.
fn wanted_cache_mode() -> CacheMode {
    if crate::guard::is_guard_enabled() {
        return CacheMode::Bypassed;
    }
    if crate::handler_config::checks_compiled_out() {
        return CacheMode::Cached;
    }
    let is_observed = crate::handler_config::is_leak_check_enabled()
        || crate::handler_config::is_alloc_count_enabled()
        || crate::rc_stats::is_enabled()
        || std::env::var_os("MallocScribble").is_some();
    if is_observed {
        CacheMode::Poisoned
    } else {
        CacheMode::Cached
    }
}

/// Records `base` as parked: its sentinel, and in a poisoned cache its fill.
///
/// # Safety
/// `base` must be a released block of `size` bytes, at least `PARKED_HEADER`.
#[inline(always)]
unsafe fn mark_parked(base: *mut u8, size: usize, mode: CacheMode) {
    // SAFETY: the block holds at least two words.
    unsafe { base.cast::<usize>().add(1).write(PARKED_SENTINEL) };
    if mode == CacheMode::Poisoned {
        // SAFETY: guaranteed by the caller.
        unsafe { fill_released(base, size) };
    }
}

/// Fills a released block past its header with [`RELEASED_FILL`].
///
/// # Safety
/// `base` must be a released block of `size` bytes, at least `PARKED_HEADER`.
#[cold]
unsafe fn fill_released(base: *mut u8, size: usize) {
    // SAFETY: the bytes past the header lie inside the block.
    unsafe {
        base.add(PARKED_HEADER)
            .write_bytes(RELEASED_FILL, size - PARKED_HEADER)
    };
}

/// Checks that `block` was left alone while parked, then readies it for its
/// new owner.
///
/// # Safety
/// `block` must be a block of `size` bytes, at least `PARKED_HEADER`, just
/// unlinked from a list.
#[inline(always)]
unsafe fn claim_parked(block: *mut u8, size: usize, mode: CacheMode) {
    // SAFETY: the block holds at least two words.
    let marker = unsafe { block.cast::<usize>().add(1) };
    // SAFETY: as above.
    if unsafe { marker.read() } != PARKED_SENTINEL {
        trap_write_after_release();
    }
    // SAFETY: as above.
    unsafe { marker.write(0) };
    if mode == CacheMode::Poisoned {
        // SAFETY: guaranteed by the caller.
        unsafe { claim_poisoned(block, size) };
    }
}

/// Checks a poisoned block's fill is all there, then fills the block for its
/// new owner with [`HANDED_OUT_FILL`].
///
/// # Safety
/// `block` must be a block of `size` bytes, at least `PARKED_HEADER`, that no
/// one else holds.
#[cold]
unsafe fn claim_poisoned(block: *mut u8, size: usize) {
    // SAFETY: the bytes past the header lie inside the block.
    let fill =
        unsafe { std::slice::from_raw_parts(block.add(PARKED_HEADER), size - PARKED_HEADER) };
    if !fill.iter().all(|&byte| byte == RELEASED_FILL) {
        trap_write_after_release();
    }
    // SAFETY: the whole block belongs to its new owner.
    unsafe { block.write_bytes(HANDED_OUT_FILL, size) };
}

#[cold]
fn trap_write_after_release() -> ! {
    trap(
        code::HEAP_CORRUPTED,
        "a heap block was written after it was released",
    )
}

/// Whether `base`, being released, already carries the parked sentinel: the
/// block was released before and is on a list.
///
/// # Safety
/// `base` must point to at least two readable words.
#[inline(always)]
unsafe fn is_already_parked(base: *mut u8) -> bool {
    // SAFETY: guaranteed by the caller.
    unsafe { base.cast::<usize>().add(1).read() == PARKED_SENTINEL }
}

/// The bytes the system really gave `base`, where the platform can say.
fn system_block_size(base: *mut u8) -> Option<usize> {
    #[cfg(target_os = "macos")]
    // SAFETY: `base` came from the system allocator, which is `malloc` for
    // every layout this module asks for.
    return Some(unsafe { libc::malloc_size(base.cast()) });
    #[cfg(target_os = "linux")]
    // SAFETY: as above.
    return Some(unsafe { libc::malloc_usable_size(base.cast()) });
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = base;
        None
    }
}

/// Refuses, in a poisoned cache, a release that names a class larger than the
/// block: parking it there would hand its owner bytes past its end.
#[cold]
fn check_released_size(base: *mut u8, class: usize) {
    if system_block_size(base).is_some_and(|size| size < class_size(class)) {
        trap(
            code::HEAP_CORRUPTED,
            "a heap block was released at a size larger than it was allocated at",
        );
    }
}

/// The size class a block of `size` bytes belongs to, or `None` when blocks of
/// that size are not cached.
#[inline(always)]
fn cached_class(size: usize, mode: CacheMode) -> Option<usize> {
    if size == 0 || size > LARGEST_CACHED_BLOCK || mode == CacheMode::Bypassed {
        return None;
    }
    Some((size - 1) / CLASS_GRANULE)
}

/// Allocates `size` bytes from the system, or null when `size` is zero, too
/// large for a layout, or the system has no memory.
fn take_from_system(size: usize) -> *mut u8 {
    if size == 0 {
        return std::ptr::null_mut();
    }
    let Ok(layout) = Layout::from_size_align(size, BLOCK_ALIGN) else {
        return std::ptr::null_mut();
    };
    // SAFETY: `layout` has a non-zero size.
    unsafe { alloc(layout) }
}

/// Returns a block of `size` bytes, allocated by [`take_from_system`], to the
/// system.
///
/// # Safety
/// `base` must come from [`take_from_system`] called with this `size`.
unsafe fn release_to_system(base: *mut u8, size: usize) {
    if let Ok(layout) = Layout::from_size_align(size, BLOCK_ALIGN) {
        // SAFETY: `base` was allocated with this same layout.
        unsafe { dealloc(base, layout) };
    }
}

/// A new block of `class`, filled for its owner in a poisoned cache.
///
/// Its second word is cleared either way: memory a parked block once occupied
/// can come back from the system still carrying the sentinel, and an owner that
/// never writes that word would then look like a second release.
fn take_new_block(class: usize, mode: CacheMode) -> *mut u8 {
    let block = take_from_system(class_size(class));
    if block.is_null() {
        return block;
    }
    if mode == CacheMode::Poisoned {
        // SAFETY: the block is `class_size(class)` bytes and freshly allocated.
        unsafe { block.write_bytes(HANDED_OUT_FILL, class_size(class)) };
    }
    // SAFETY: every class holds at least two words.
    unsafe { block.cast::<usize>().add(1).write(0) };
    block
}

/// Allocates a block of at least `size` bytes, aligned to [`BLOCK_ALIGN`],
/// whose contents are unspecified: a reused block holds what its previous
/// owner left in it. Returns null when `size` is zero or the system has no
/// memory.
///
/// # Safety
/// The block must be released with [`release_block`] given the same `size`.
#[inline(always)]
pub unsafe fn alloc_block(size: usize) -> *mut u8 {
    let mode = cache_mode();
    let Some(class) = cached_class(size, mode) else {
        return take_from_system(size);
    };
    let parked = this_thread_cache().take(class, mode);
    match parked {
        Some(block) => {
            count_reuse(mode);
            block
        }
        None => take_new_block(class, mode),
    }
}

/// Releases a block [`alloc_block`] returned.
///
/// # Safety
/// `base` must come from [`alloc_block`] called with this `size`, and must not
/// be used after this call.
#[inline(always)]
pub unsafe fn release_block(base: *mut u8, size: usize) {
    let mode = cache_mode();
    let Some(class) = cached_class(size, mode) else {
        // SAFETY: an uncached block was allocated at exactly `size`.
        return unsafe { release_to_system(base, size) };
    };
    // SAFETY: a cached block holds at least `CLASS_GRANULE` bytes.
    if unsafe { is_already_parked(base) } {
        trap_released_twice();
    }
    if mode == CacheMode::Poisoned {
        check_released_size(base, class);
    }
    debug_assert!(
        system_block_size(base).is_none_or(|size| size >= class_size(class)),
        "a block released at a class larger than the block"
    );
    if !this_thread_cache().park(base, class, mode) {
        // SAFETY: a cached block was allocated at its class size.
        unsafe { release_to_system(base, class_size(class)) };
    }
}

#[cold]
fn trap_released_twice() -> ! {
    trap(code::HEAP_CORRUPTED, "a heap block was released twice")
}

/// Counts, for `MIRI_ALLOC_COUNT`, a block handed out again. Only a poisoned
/// cache counts: it is the one in use whenever a counter is asked for.
fn count_reuse(mode: CacheMode) {
    if mode == CacheMode::Poisoned {
        crate::alloc_count::increment_reused_count();
    }
}

/// Stable FFI interface for blocks compiled code allocates itself.
pub mod ffi {
    use super::{alloc_block, release_block};

    /// Bytes the block's first word, which holds its size, takes.
    const SIZE_WORD: usize = std::mem::size_of::<usize>();

    /// Allocates the `size`-byte block of a value compiled code lays out
    /// itself — a class instance, a tuple, an option, an enum payload, a
    /// closure environment — and records its size, at least one word, in its
    /// first word, which [`miri_rt_block_free`] reads back. Every other byte
    /// is unspecified, so compiled code writes all of them it reads. Returns
    /// null when the system has no memory.
    ///
    /// # Safety
    /// The block must be released with [`miri_rt_block_free`] alone, and its
    /// first word must not be written while it is live.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_block_alloc(size: usize) -> *mut u8 {
        let size = size.max(SIZE_WORD);
        // SAFETY: released by `miri_rt_block_free` with the size stored here.
        let base = unsafe { alloc_block(size) };
        if !base.is_null() {
            // SAFETY: the block holds at least one word.
            unsafe { base.cast::<usize>().write(size) };
        }
        base
    }

    /// Releases a block [`miri_rt_block_alloc`] returned. Null is ignored.
    ///
    /// # Safety
    /// `base` must be null or a live block from [`miri_rt_block_alloc`], and
    /// must not be used after this call.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_block_free(base: *mut u8) {
        if base.is_null() {
            return;
        }
        // TODO: a parked block's first word is its free-list link, not a size,
        // so releasing one twice through here reads a pointer as its size and
        // hands it to the system allocator instead of trapping. Compiled code
        // cannot reach this (it decrements the count in the second word first,
        // and the parked marker never reaches zero), but a runtime caller could;
        // checking `is_already_parked` before reading the size would close it.
        // SAFETY: a live block's first word holds the size it was allocated at.
        unsafe {
            let size = base.cast::<usize>().read();
            release_block(base, size);
        }
    }
}

/// How many blocks of the size class holding `size`-byte blocks this thread
/// has parked.
#[cfg(test)]
pub(crate) fn parked_count(size: usize) -> usize {
    let class = (size - 1) / CLASS_GRANULE;
    CACHE.with(|cache| {
        let mut length = 0;
        let mut block = cache.heads[class].get();
        while !block.is_null() {
            length += 1;
            // SAFETY: every listed block carries the link a push wrote.
            block = unsafe { (*block).next };
        }
        length
    })
}

/// How many bytes this thread has parked.
#[cfg(test)]
pub(crate) fn parked_bytes() -> usize {
    CACHE.with(|cache| cache.parked_bytes.get())
}

#[cfg(test)]
mod tests;
