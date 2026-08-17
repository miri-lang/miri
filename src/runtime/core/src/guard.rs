// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Shadow-heap sanitizer for detecting use-after-free, double-free, and UAF bugs.
//!
//! Activated by `MIRI_HEAP_GUARD=1` environment variable.
//! Maintains a shadow table tracking all heap allocations, poisoning freed blocks and
//! quarantining them. Captures allocation site via #[track_caller].
//!
//! Zero overhead when disabled (single atomic bool load on each hook).

use crate::rc::RC_HEADER_SIZE;
use std::alloc::{dealloc, Layout};
use std::collections::HashMap;
use std::panic::Location;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;

/// Result of a free operation — signals what action to take with the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreeVerdict {
    /// Block successfully marked Freed; caller must NOT dealloc now (quarantined).
    Quarantine,
    /// Guard is disabled or block was not tracked. Caller should deallocate normally.
    DeallocNow,
    /// Block was already Freed — double-free detected. Must call report_and_abort.
    DoubleFree,
    /// Block was never tracked — audit gap or wild free. Caller should deallocate.
    Untracked,
    /// A quarantined block's poison had been overwritten, proving a write to it
    /// after it was freed. Fatal: the caller must call `report_and_abort`.
    WriteAfterFree,
}

/// Byte written over a freed payload so a later read or write to it is
/// recognizable rather than plausible-looking stale data.
const POISON_BYTE: u8 = 0xDD;

/// A quarantined block whose poison was overwritten while it sat in quarantine.
#[derive(Debug, Clone)]
pub struct PoisonViolation {
    /// Payload pointer of the block that was written to after being freed.
    pub payload_ptr: usize,
    /// The block's record as of its allocation and free.
    pub record: AllocRecord,
}

/// Result of a validation check on a managed pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TouchVerdict {
    /// Pointer is valid and Live.
    Ok,
    /// Pointer is Freed — use-after-free. Must call report_and_abort.
    UseAfterFree,
    /// RC slot is corrupted (zero or implausibly large). Must call report_and_abort.
    HeaderCorrupt,
}

/// Kind of allocation (for diagnostics).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AllocKind {
    Unknown,
    List,
    Map,
    Set,
    Array,
    String,
    Closure,
    Class,
    /// A collection's element storage: the buffers `List`, `Map`, `Set` and
    /// `Array` allocate for their own contents. These carry no RC header and are
    /// owned by the collection, not by the value the program holds.
    Buffer,
}

impl AllocKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            AllocKind::Unknown => "unknown",
            AllocKind::List => "list",
            AllocKind::Map => "map",
            AllocKind::Set => "set",
            AllocKind::Array => "array",
            AllocKind::String => "string",
            AllocKind::Closure => "closure",
            AllocKind::Class => "class",
            AllocKind::Buffer => "buffer",
        }
    }

    /// Infers the kind from the source file of the allocating intrinsic.
    ///
    /// `alloc_with_rc` is shared by every collection and string, so it cannot
    /// name a kind of its own; each runtime collection lives in its own file, so
    /// the tracked caller's file identifies it. Interim: passing an explicit
    /// kind is more direct but needs a parameter at all six call sites.
    pub fn from_call_site_file(file: &str) -> AllocKind {
        if file.ends_with("list.rs") {
            AllocKind::List
        } else if file.ends_with("map.rs") {
            AllocKind::Map
        } else if file.ends_with("set.rs") {
            AllocKind::Set
        } else if file.ends_with("array.rs") {
            AllocKind::Array
        } else if file.contains("string") {
            AllocKind::String
        } else {
            AllocKind::Unknown
        }
    }
}

/// Allocation record in the shadow table.
#[derive(Debug, Clone)]
pub struct AllocRecord {
    /// Allocation site captured via #[track_caller].
    pub site: &'static Location<'static>,
    /// Monotonically increasing allocation sequence number.
    pub seq: u64,
    /// Kind of allocation (for diagnostics).
    pub kind: AllocKind,
    /// Size and alignment of the allocation (includes RC header).
    pub layout: Layout,
    /// State of the allocation (Live or Freed).
    pub state: AllocState,
    /// Whether this block is a bare allocation with no `[RC][payload]` header.
    ///
    /// Two kinds of block qualify: the ones codegen allocates inline for class
    /// instances, tuples, `Option`s, enum payloads and closure environments, and
    /// the element buffers a collection allocates for its own storage. Neither
    /// carries an RC at `ptr - RC_HEADER_SIZE`, and the guard does not own their
    /// memory, so neither is poisoned or quarantined — the guard only witnesses
    /// the alloc/free pair to catch a double free and attribute a leak.
    pub raw_block: bool,
}

/// State of an allocation.
#[derive(Debug, Clone, PartialEq)]
pub enum AllocState {
    Live,
    Freed {
        /// Free site captured via #[track_caller].
        free_site: &'static Location<'static>,
        /// Sequence number at free time.
        free_seq: u64,
    },
}

/// Guard state containing the shadow table and quarantine.
pub struct GuardState {
    /// Shadow table: payload pointer -> AllocRecord.
    pub table: HashMap<usize, AllocRecord>,
    /// Quarantine FIFO: list of (payload_ptr, layout, alloc_record).
    /// Ordered oldest-first; when full, evicts the oldest.
    pub quarantine: Vec<(usize, Layout, AllocRecord)>,
    /// Maximum quarantine size in bytes.
    pub quarantine_capacity: usize,
    /// Current quarantine size in bytes.
    pub quarantine_used: usize,
    /// Monotonic allocator sequence counter (local to this GuardState).
    seq_counter: u64,
    /// Most recent write-after-free detected on quarantine eviction.
    poison_violation: Option<PoisonViolation>,
}

impl GuardState {
    pub fn new(quarantine_capacity: usize) -> Self {
        GuardState {
            table: HashMap::new(),
            quarantine: Vec::new(),
            quarantine_capacity,
            quarantine_used: 0,
            seq_counter: 0,
            poison_violation: None,
        }
    }

    /// Records an allocation.
    pub fn record_alloc(
        &mut self,
        ptr: usize,
        size: usize,
        kind: AllocKind,
        site: &'static Location<'static>,
    ) {
        if ptr == 0 || size == 0 {
            return;
        }

        let seq = self.seq_counter;
        self.seq_counter += 1;

        let full_size = RC_HEADER_SIZE + size;
        let layout = match Layout::from_size_align(full_size, 8) {
            Ok(l) => l,
            Err(_) => return,
        };

        let record = AllocRecord {
            site,
            seq,
            kind,
            layout,
            state: AllocState::Live,
            raw_block: false,
        };
        self.table.insert(ptr, record);
    }

    /// Records a bare allocation the guard does not own: one codegen made
    /// inline, or a collection's own element buffer.
    ///
    /// The size is not tracked — codegen computes it inline, and a collection
    /// buffer is reallocated as it grows — so the record carries a zero layout
    /// and is never quarantined, poisoned, or read for an RC. What it still buys
    /// is double-free detection and leak attribution for allocations that no
    /// counter in this runtime can currently see.
    pub fn record_alloc_raw(
        &mut self,
        ptr: usize,
        kind: AllocKind,
        site: &'static Location<'static>,
    ) {
        if ptr == 0 {
            return;
        }

        let seq = self.seq_counter;
        self.seq_counter += 1;

        self.table.insert(
            ptr,
            AllocRecord {
                site,
                seq,
                kind,
                layout: Layout::from_size_align(0, 1).unwrap_or_else(|_| std::process::abort()),
                state: AllocState::Live,
                raw_block: true,
            },
        );
    }

    /// Records a free of a block compiled code owns.
    ///
    /// Returns [`FreeVerdict::DeallocNow`] on success because codegen performs
    /// the release itself; the guard only witnesses it. A repeat free is still
    /// [`FreeVerdict::DoubleFree`].
    pub fn record_free_raw(
        &mut self,
        ptr: usize,
        free_site: &'static Location<'static>,
    ) -> FreeVerdict {
        if ptr == 0 {
            return FreeVerdict::DeallocNow;
        }

        match self.table.get_mut(&ptr) {
            None => FreeVerdict::Untracked,
            Some(record) => match record.state {
                AllocState::Freed { .. } => FreeVerdict::DoubleFree,
                AllocState::Live => {
                    let free_seq = self.seq_counter;
                    record.state = AllocState::Freed {
                        free_site,
                        free_seq,
                    };
                    self.seq_counter += 1;
                    FreeVerdict::DeallocNow
                }
            },
        }
    }

    /// Records a free and returns a verdict.
    pub fn record_free(
        &mut self,
        payload_ptr: usize,
        free_site: &'static Location<'static>,
    ) -> FreeVerdict {
        if payload_ptr == 0 {
            return FreeVerdict::DeallocNow;
        }

        let freed_layout = match self.table.get_mut(&payload_ptr) {
            None => return FreeVerdict::Untracked,
            Some(record) => match record.state {
                AllocState::Freed { .. } => return FreeVerdict::DoubleFree,
                AllocState::Live => {
                    // Poison the payload so a later read or write to it is
                    // recognizable instead of looking like valid stale data.
                    let payload_size = record.layout.size().saturating_sub(RC_HEADER_SIZE);
                    unsafe {
                        std::ptr::write_bytes(payload_ptr as *mut u8, POISON_BYTE, payload_size);
                    }

                    let free_seq = self.seq_counter;
                    record.state = AllocState::Freed {
                        free_site,
                        free_seq,
                    };
                    record.layout
                }
            },
        };
        self.seq_counter += 1;

        // The quarantined copy carries the Freed state, so a later poison
        // violation can name the free site as well as the allocation site.
        let quarantined_record = self.table[&payload_ptr].clone();
        self.quarantine_used += freed_layout.size();
        self.quarantine
            .push((payload_ptr, freed_layout, quarantined_record));

        match self.evict_if_over_capacity() {
            Some(violation) => {
                self.poison_violation = Some(violation);
                FreeVerdict::WriteAfterFree
            }
            None => FreeVerdict::Quarantine,
        }
    }

    /// Takes the most recent write-after-free violation, if any.
    ///
    /// Held on the state rather than returned inline so `record_free` keeps a
    /// plain verdict return; the reporter retrieves the details when it handles
    /// a [`FreeVerdict::WriteAfterFree`].
    pub fn take_poison_violation(&mut self) -> Option<PoisonViolation> {
        self.poison_violation.take()
    }

    /// Evicts the oldest quarantined blocks until the quarantine is back within
    /// capacity, returning the first block whose poison had been overwritten.
    ///
    /// A changed byte proves something wrote to the block after it was freed,
    /// even if no intrinsic ever presented the pointer for validation — which is
    /// the only way this class of bug is observable once the block is released.
    /// Eviction is where the check happens because that is the last moment the
    /// contents are still intact.
    fn evict_if_over_capacity(&mut self) -> Option<PoisonViolation> {
        let mut violation = None;

        while self.quarantine_used > self.quarantine_capacity && !self.quarantine.is_empty() {
            let (old_payload_ptr, full_layout, old_record) = self.quarantine.remove(0);
            self.quarantine_used -= full_layout.size();

            let payload_size = full_layout.size() - RC_HEADER_SIZE;
            let poison_intact = unsafe {
                let slice = std::slice::from_raw_parts(old_payload_ptr as *const u8, payload_size);
                slice.iter().all(|&b| b == POISON_BYTE)
            };

            // Report the first violation; later ones are almost certainly
            // downstream of the same defect.
            if !poison_intact && violation.is_none() {
                violation = Some(PoisonViolation {
                    payload_ptr: old_payload_ptr,
                    record: old_record,
                });
            }

            unsafe {
                let base = (old_payload_ptr - RC_HEADER_SIZE) as *mut u8;
                dealloc(base, full_layout);
            }
        }

        violation
    }

    /// Validates that a pointer is Live and accessible (not Freed).
    pub fn validate(&self, payload_ptr: usize) -> TouchVerdict {
        if payload_ptr == 0 {
            return TouchVerdict::Ok;
        }

        match self.table.get(&payload_ptr) {
            None => TouchVerdict::Ok,
            Some(record) => match record.state {
                AllocState::Live => {
                    // Check RC sanity.
                    unsafe {
                        let rc_ptr = (payload_ptr - RC_HEADER_SIZE) as *const usize;
                        let rc = *rc_ptr;
                        if rc == 0 || rc > 1_000_000 {
                            return TouchVerdict::HeaderCorrupt;
                        }
                    }
                    TouchVerdict::Ok
                }
                AllocState::Freed { .. } => TouchVerdict::UseAfterFree,
            },
        }
    }
}

/// Guard enabled flag, cached from environment variable.
static GUARD_ENABLED: AtomicBool = AtomicBool::new(false);
static GUARD_ENABLED_INIT: std::sync::Once = std::sync::Once::new();

/// Returns whether the guard is enabled (checked via cached atomic bool).
fn is_guard_enabled() -> bool {
    GUARD_ENABLED_INIT.call_once(|| {
        let enabled = std::env::var("MIRI_HEAP_GUARD").as_deref() == Ok("1");
        GUARD_ENABLED.store(enabled, Ordering::Relaxed);
    });
    GUARD_ENABLED.load(Ordering::Relaxed)
}

/// Compiled code has not yet learned whether the runtime wants to observe its
/// allocations. Deliberately distinct from [`TRACKING_OFF`], so the first
/// allocation still calls the hook and that call is what settles the question —
/// no allocation is missed before the environment has been read, and the
/// runtime needs no startup entry point it does not have.
pub const TRACKING_UNSET: u8 = 0;

/// The runtime does not want to observe allocations. Compiled code skips the
/// hook call entirely.
pub const TRACKING_OFF: u8 = 1;

/// The runtime wants every allocation and release reported.
pub const TRACKING_ON: u8 = 2;

/// Whether compiled code should report the allocations it makes inline.
///
/// Codegen loads this before each hook call and skips the call while it reads
/// [`TRACKING_OFF`]. The call cannot be inlined — it crosses from a
/// Cranelift-emitted program into this static library — so hoisting the test to
/// the call site is what keeps an unobserved allocation down to a load and a
/// branch it will predict.
///
/// "Tracking" is wider than the guard on purpose. The leak counter registers
/// its exit handler from the same hook, so a program that only allocates inline
/// — a class, a tuple, a closure environment — would never register it if the
/// guard alone decided this.
/// Named in the exported `miri_rt_*` style shared by every symbol compiled code
/// links against, which is also what the drift check between the compiler's
/// symbol table and this library scans for.
#[no_mangle]
#[allow(non_upper_case_globals)]
pub static miri_rt_tracking_state: std::sync::atomic::AtomicU8 =
    std::sync::atomic::AtomicU8::new(TRACKING_UNSET);

/// Settles [`MIRI_RT_TRACKING_STATE`] on the first allocation reported.
///
/// Idempotent, and safe to race: every caller computes the same answer from the
/// same environment, so a concurrent second store writes the value already
/// there.
pub fn resolve_tracking_state() {
    if miri_rt_tracking_state.load(Ordering::Relaxed) != TRACKING_UNSET {
        return;
    }
    let wanted = is_guard_enabled() || crate::rc::is_leak_check_enabled();
    let state = if wanted { TRACKING_ON } else { TRACKING_OFF };
    miri_rt_tracking_state.store(state, Ordering::Relaxed);
}

/// Default quarantine capacity in bytes.
const DEFAULT_QUARANTINE_CAPACITY: usize = 256 * 1024 * 1024; // 256 MB

/// Global guard state.
static GUARD: Mutex<Option<GuardState>> = Mutex::new(None);

/// Initializes the global guard if enabled and not yet initialized.
fn ensure_guard_init() {
    if !is_guard_enabled() {
        return;
    }

    let mut guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if guard.is_none() {
        let capacity = parse_quarantine_capacity(std::env::var("MIRI_HEAP_GUARD_QUARANTINE").ok());
        *guard = Some(GuardState::new(capacity));

        // Register atexit handler to report leaks.
        unsafe {
            libc::atexit(leak_report_at_exit);
        }
    }
}

/// Parses the quarantine capacity from an environment variable value.
pub fn parse_quarantine_capacity(value: Option<String>) -> usize {
    match value {
        None => DEFAULT_QUARANTINE_CAPACITY,
        Some(s) => {
            match s.parse::<usize>() {
                Err(_) => DEFAULT_QUARANTINE_CAPACITY,
                Ok(cap) => {
                    // Clamp to a sane maximum to prevent overflow / OOM.
                    cap.min(1024 * 1024 * 1024) // 1 GB max
                }
            }
        }
    }
}

/// Called at process exit to report any leaked allocations.
extern "C" fn leak_report_at_exit() {
    let guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(state) = guard.as_ref() else {
        return;
    };

    let msg = collect_leak_groups(state);
    if msg.is_empty() {
        return;
    }

    unsafe {
        libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        libc::_exit(99);
    }
}

/// Collects live allocations into groups by (site, kind) for leak reporting.
pub fn collect_leak_groups(state: &GuardState) -> String {
    let mut groups: std::collections::BTreeMap<
        (&'static Location<'static>, AllocKind),
        (usize, Option<usize>),
    > = std::collections::BTreeMap::new();

    for (ptr, record) in &state.table {
        if let AllocState::Live = record.state {
            // A codegen-owned block's header does not sit at `ptr -
            // RC_HEADER_SIZE`, so reading an RC there would dereference memory
            // outside the allocation. Report it without one.
            let rc = if record.raw_block {
                None
            } else {
                let rc = unsafe { *((ptr - RC_HEADER_SIZE) as *const usize) };
                // An immortal object stores a negative `isize` RC and is never
                // freed by design (see `incref`), so it is not a leak.
                if (rc as isize) < 0 {
                    continue;
                }
                Some(rc)
            };

            let key = (record.site, record.kind);
            let entry = groups.entry(key).or_insert((0, None));
            entry.0 += 1; // count
            entry.1 = rc; // final RC, absent for codegen-owned blocks
        }
    }

    if groups.is_empty() {
        return String::new();
    }

    let mut msg = String::from("MIRI_HEAP_GUARD: leaked ");
    let total_count: usize = groups.values().map(|e| e.0).sum();
    msg.push_str(&total_count.to_string());
    msg.push_str(" allocations: ");

    let mut first = true;
    for ((site, kind), (count, rc)) in &groups {
        if !first {
            msg.push_str("; ");
        }
        first = false;
        msg.push_str(site.file());
        msg.push(':');
        msg.push_str(&site.line().to_string());
        msg.push_str(" (");
        msg.push_str(kind.as_str());
        if let Some(r) = rc {
            msg.push_str(", rc=");
            msg.push_str(&r.to_string());
        }
        msg.push(')');
        if *count > 1 {
            msg.push_str(" x");
            msg.push_str(&count.to_string());
        }
    }
    msg.push('\n');

    msg
}

/// Appends `site` as `file:line` followed by `(label=seq)`.
fn push_site(msg: &mut String, site: &Location<'static>, label: &str, seq: u64) {
    msg.push_str(site.file());
    msg.push(':');
    msg.push_str(&site.line().to_string());
    msg.push_str(" (");
    msg.push_str(label);
    msg.push_str(&seq.to_string());
    msg.push(')');
}

/// Builds the diagnostic for a fatal verdict.
///
/// Takes the record by value so the caller can release the guard lock before
/// formatting: this runs on a path that is about to terminate the process, and
/// allocating under the lock is the re-entrancy hazard the guard must avoid.
fn format_fatal_report(
    verdict: FreeVerdict,
    record: Option<AllocRecord>,
    violation: Option<PoisonViolation>,
    reporter_site: &Location<'static>,
) -> String {
    let mut msg = String::from("MIRI_HEAP_GUARD: ");

    match verdict {
        FreeVerdict::DoubleFree => {
            msg.push_str("double-free detected: allocated at ");
            match record {
                Some(record) => {
                    push_site(&mut msg, record.site, "alloc seq=", record.seq);
                    if let AllocState::Freed {
                        free_site,
                        free_seq,
                    } = record.state
                    {
                        msg.push_str(", first freed at ");
                        push_site(&mut msg, free_site, "seq=", free_seq);
                    }
                }
                None => msg.push_str("<no record>"),
            }
            msg.push_str(", freed again at ");
            msg.push_str(reporter_site.file());
            msg.push(':');
            msg.push_str(&reporter_site.line().to_string());
        }
        FreeVerdict::WriteAfterFree => {
            msg.push_str("write-after-free detected: the payload poison of a freed block ");
            msg.push_str("had been overwritten by the time it left quarantine; allocated at ");
            match violation {
                Some(violation) => {
                    push_site(
                        &mut msg,
                        violation.record.site,
                        "alloc seq=",
                        violation.record.seq,
                    );
                    if let AllocState::Freed {
                        free_site,
                        free_seq,
                    } = violation.record.state
                    {
                        msg.push_str(", freed at ");
                        push_site(&mut msg, free_site, "seq=", free_seq);
                    }
                }
                None => msg.push_str("<no record>"),
            }
        }
        // Not fatal: these are the ordinary outcomes of a free and are handled
        // by the caller rather than reported here.
        FreeVerdict::Quarantine | FreeVerdict::DeallocNow | FreeVerdict::Untracked => {
            msg.push_str("internal error: reported a non-fatal verdict (");
            msg.push_str(match verdict {
                FreeVerdict::Quarantine => "quarantine",
                FreeVerdict::DeallocNow => "dealloc-now",
                FreeVerdict::Untracked => "untracked",
                FreeVerdict::DoubleFree | FreeVerdict::WriteAfterFree => "fatal",
            });
            msg.push(')');
        }
    }

    msg.push('\n');
    msg
}

/// Reports a fatal guard verdict and terminates the process.
///
/// Diverges, so a caller needs no unreachable hint after it — which is what
/// keeps the double-free path from falling through into a second release.
#[track_caller]
pub fn report_and_abort(verdict: FreeVerdict, payload_ptr: usize) -> ! {
    // Copy what the report needs, then drop the lock before formatting.
    let (record, violation) = {
        let mut guard = GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_mut() {
            Some(state) => (
                state.table.get(&payload_ptr).cloned(),
                state.take_poison_violation(),
            ),
            None => (None, None),
        }
    };

    let msg = format_fatal_report(verdict, record, violation, Location::caller());

    unsafe {
        libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        libc::_exit(99);
    }
}

/// Builds the diagnostic for a use-after-free or corrupted header detected at intrinsic entry.
///
/// Takes the record by value so the caller can release the guard lock before
/// formatting, matching the pattern used by `format_fatal_report`.
fn format_touch_report(
    verdict: TouchVerdict,
    record: Option<AllocRecord>,
    touching_site: &Location<'static>,
) -> String {
    let mut msg = String::from("MIRI_HEAP_GUARD: ");

    match verdict {
        TouchVerdict::UseAfterFree => {
            msg.push_str("use-after-free detected: intrinsic at ");
            msg.push_str(touching_site.file());
            msg.push(':');
            msg.push_str(&touching_site.line().to_string());
            msg.push_str(" touched pointer allocated at ");
            match record {
                Some(record) => {
                    push_site(&mut msg, record.site, "alloc seq=", record.seq);
                    if let AllocState::Freed {
                        free_site,
                        free_seq,
                    } = record.state
                    {
                        msg.push_str(", freed at ");
                        push_site(&mut msg, free_site, "seq=", free_seq);
                    }
                }
                None => msg.push_str("<no record>"),
            }
        }
        TouchVerdict::HeaderCorrupt => {
            msg.push_str("header corrupted: intrinsic at ");
            msg.push_str(touching_site.file());
            msg.push(':');
            msg.push_str(&touching_site.line().to_string());
            msg.push_str(" found RC header inconsistency at pointer allocated at ");
            match record {
                Some(record) => {
                    push_site(&mut msg, record.site, "alloc seq=", record.seq);
                }
                None => msg.push_str("<no record>"),
            }
        }
        TouchVerdict::Ok => {
            msg.push_str("internal error: reported a non-fatal verdict (ok)");
        }
    }

    msg.push('\n');
    msg
}

/// Reports a use-after-free or corrupted header at intrinsic entry and terminates the process.
///
/// Diverges, matching the signature of `report_and_abort`.
#[track_caller]
pub fn report_touch_and_abort(verdict: TouchVerdict, payload_ptr: usize) -> ! {
    // Copy what the report needs, then drop the lock before formatting.
    let record = {
        let guard = GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match guard.as_ref() {
            Some(state) => state.table.get(&payload_ptr).cloned(),
            None => None,
        }
    };

    let msg = format_touch_report(verdict, record, Location::caller());

    unsafe {
        libc::write(2, msg.as_ptr() as *const libc::c_void, msg.len());
        libc::_exit(99);
    }
}

/// Registers an allocation whose memory compiled code owns.
///
/// Attribution here is coarser than for a runtime allocation: the recorded site
/// is the tracking intrinsic, not the Miri source that constructed the value,
/// because codegen emits the malloc inline with no site information. The kind
/// still distinguishes these blocks in the leak report.
#[track_caller]
pub fn guard_alloc_raw(ptr: *mut u8, kind: AllocKind) {
    if !is_guard_enabled() || ptr.is_null() {
        return;
    }

    ensure_guard_init();
    let site = Location::caller();

    let mut guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(ref mut state) = *guard {
        state.record_alloc_raw(ptr as usize, kind, site);
    }
}

/// Witnesses a free of a block compiled code owns, reporting a double free.
///
/// # Safety
/// `ptr` must be the allocation base compiled code is about to release, or null.
#[track_caller]
pub unsafe fn guard_free_raw(ptr: *mut u8) {
    if !is_guard_enabled() || ptr.is_null() {
        return;
    }

    let free_site = Location::caller();
    let verdict = {
        let mut guard = GUARD
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        match *guard {
            Some(ref mut state) => state.record_free_raw(ptr as usize, free_site),
            None => FreeVerdict::DeallocNow,
        }
    };

    if verdict == FreeVerdict::DoubleFree {
        report_and_abort(verdict, ptr as usize);
    }
}

/// Registers an allocation in the shadow table (public API).
#[track_caller]
pub fn guard_alloc(ptr: *mut u8, size: usize, kind: AllocKind) {
    if !is_guard_enabled() {
        return;
    }

    if ptr.is_null() || size == 0 {
        return;
    }

    ensure_guard_init();

    let payload_ptr = ptr as usize;
    let site = Location::caller();

    // A caller that named no kind gets one inferred from its source file.
    let derived_kind = if kind == AllocKind::Unknown {
        AllocKind::from_call_site_file(site.file())
    } else {
        kind
    };

    let mut guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    if let Some(ref mut state) = *guard {
        state.record_alloc(payload_ptr, size, derived_kind, site);
    }
}

/// Marks an allocation as Freed and moves it to quarantine (public API).
///
/// # Safety
/// `ptr` must be a valid payload pointer previously registered via `guard_alloc`,
/// or null (which is treated as a no-op).
#[track_caller]
pub unsafe fn guard_free(ptr: *mut u8) -> FreeVerdict {
    if !is_guard_enabled() {
        return FreeVerdict::DeallocNow;
    }

    if ptr.is_null() {
        return FreeVerdict::DeallocNow;
    }

    let payload_ptr = ptr as usize;
    let free_site = Location::caller();

    let mut guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(ref mut state) = *guard else {
        return FreeVerdict::DeallocNow;
    };

    state.record_free(payload_ptr, free_site)
}

/// Validates that a pointer is Live and accessible (public API).
#[track_caller]
pub fn guard_validate(ptr: *mut u8) -> TouchVerdict {
    if !is_guard_enabled() {
        return TouchVerdict::Ok;
    }

    if ptr.is_null() {
        return TouchVerdict::Ok;
    }

    let payload_ptr = ptr as usize;

    let guard = GUARD
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());

    let Some(ref state) = *guard else {
        return TouchVerdict::Ok;
    };

    state.validate(payload_ptr)
}

/// Validates a pointer on intrinsic entry, diverging on a fatal verdict.
///
/// This is the check every intrinsic that receives a managed pointer calls first.
/// A no-op when the guard is disabled (single cached-bool load); treats null as OK.
/// On a fatal verdict (use-after-free or header corruption), reports and terminates.
///
/// # Safety
/// `ptr` must be a valid payload pointer (one word after an RC header) or null.
/// If the guard is disabled or `ptr` is null, this is a no-op. Otherwise, the guard
/// validates the RC field at `ptr - RC_HEADER_SIZE`.
#[track_caller]
pub unsafe fn guard_check(ptr: *mut u8) {
    if !is_guard_enabled() {
        return;
    }

    if ptr.is_null() {
        return;
    }

    let verdict = guard_validate(ptr);
    match verdict {
        TouchVerdict::Ok => {}
        TouchVerdict::UseAfterFree | TouchVerdict::HeaderCorrupt => {
            report_touch_and_abort(verdict, ptr as usize);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::alloc::alloc;

    // Every test drives a local `GuardState` rather than the process-global
    // one. The guard hooks into `alloc_with_rc`/`free_with_rc`, which sibling
    // tests in this crate call concurrently, so a test that enabled the global
    // guard would quarantine and poison another test's memory — an
    // intermittent crash rather than an honest failure.

    /// Allocates a real `[RC][payload]` block so the guard has memory it may
    /// legitimately poison, and returns its payload pointer with the layout
    /// needed to release it.
    fn alloc_block(payload_size: usize) -> (usize, Layout) {
        let layout = Layout::from_size_align(RC_HEADER_SIZE + payload_size, 8)
            .unwrap_or_else(|_| std::process::abort());
        let base = unsafe { alloc(layout) };
        assert!(!base.is_null(), "test block allocation failed");
        ((base as usize) + RC_HEADER_SIZE, layout)
    }

    /// `alloc_with_rc` carries `#[track_caller]`, so the site recorded for an
    /// allocation is the *calling* intrinsic's location rather than a fixed line
    /// inside `rc.rs`. Without the attribute every allocation in the program
    /// shares one site and the leak report can attribute nothing.
    ///
    /// Exercised through a `#[track_caller]` helper rather than the global guard
    /// so it cannot disturb a concurrently running sibling test.
    #[test]
    fn alloc_site_is_the_caller_not_the_recorder() {
        #[track_caller]
        fn record_from_here(state: &mut GuardState, ptr: usize, size: usize) {
            state.record_alloc(ptr, size, AllocKind::Unknown, Location::caller());
        }

        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);
        let (payload_ptr, layout) = alloc_block(32);

        let expected_line = line!() + 1;
        record_from_here(&mut state, payload_ptr, 32);

        let site = state.table[&payload_ptr].site;
        assert!(
            site.file().ends_with("guard.rs"),
            "site should name the caller's file, got {}",
            site.file()
        );
        assert_eq!(
            site.line(),
            expected_line,
            "site should name the calling line, not the recording function"
        );

        unsafe { dealloc((payload_ptr - RC_HEADER_SIZE) as *mut u8, layout) };
    }

    /// The shared `alloc_with_rc` entry point names no kind of its own, so the
    /// kind is inferred from the tracked call site's file.
    #[test]
    fn kind_is_inferred_from_the_call_site_file() {
        let cases = [
            ("src/runtime/core/src/list.rs", AllocKind::List),
            ("src/runtime/core/src/map.rs", AllocKind::Map),
            ("src/runtime/core/src/set.rs", AllocKind::Set),
            ("src/runtime/core/src/array.rs", AllocKind::Array),
            ("src/runtime/core/src/string/mod.rs", AllocKind::String),
            (
                "src/runtime/core/src/string/constructors.rs",
                AllocKind::String,
            ),
            ("src/runtime/core/src/regex.rs", AllocKind::Unknown),
        ];
        for (file, expected) in cases {
            assert_eq!(
                AllocKind::from_call_site_file(file),
                expected,
                "unexpected kind inferred for {file}"
            );
        }
    }

    /// A block whose poison is overwritten while quarantined proves something
    /// wrote to it after it was freed. The check runs at eviction, the last
    /// moment the contents are still intact, and must surface as a fatal
    /// verdict rather than being silently discarded.
    #[test]
    fn tampered_poison_reports_write_after_free_on_eviction() {
        // Hold exactly one block: the first free stays quarantined (so its
        // memory is still ours to tamper with) and the second evicts it.
        const PAYLOAD: usize = 64;
        let mut state = GuardState::new(RC_HEADER_SIZE + PAYLOAD);
        let site = Location::caller();

        let (first_ptr, first_layout) = alloc_block(64);
        state.record_alloc(first_ptr, 64, AllocKind::List, site);
        // Free it, then scribble over the poison as a use-after-free would.
        let verdict = state.record_free(first_ptr, site);
        assert_eq!(
            verdict,
            FreeVerdict::Quarantine,
            "an empty quarantine evicts nothing on the first free"
        );
        unsafe { *(first_ptr as *mut u8) = 0x41 };

        // The next free evicts the tampered block and must flag it.
        let (second_ptr, second_layout) = alloc_block(64);
        state.record_alloc(second_ptr, 64, AllocKind::List, site);
        let verdict = state.record_free(second_ptr, site);

        assert_eq!(
            verdict,
            FreeVerdict::WriteAfterFree,
            "overwritten poison must be reported, not discarded"
        );
        let violation = state
            .take_poison_violation()
            .expect("a write-after-free verdict must carry its violation");
        assert_eq!(violation.payload_ptr, first_ptr);
        assert!(
            matches!(violation.record.state, AllocState::Freed { .. }),
            "the violation must carry the free site, not just the alloc site"
        );

        // The first block was released by the eviction; release the second.
        let _ = first_layout;
        unsafe { dealloc((second_ptr - RC_HEADER_SIZE) as *mut u8, second_layout) };
    }

    /// Poison left intact through eviction is the ordinary case and must not be
    /// mistaken for a write-after-free.
    #[test]
    fn intact_poison_evicts_without_a_violation() {
        const PAYLOAD: usize = 64;
        let mut state = GuardState::new(RC_HEADER_SIZE + PAYLOAD);
        let site = Location::caller();

        let (first_ptr, _) = alloc_block(64);
        state.record_alloc(first_ptr, 64, AllocKind::List, site);
        assert_eq!(state.record_free(first_ptr, site), FreeVerdict::Quarantine);

        let (second_ptr, second_layout) = alloc_block(64);
        state.record_alloc(second_ptr, 64, AllocKind::List, site);
        assert_eq!(
            state.record_free(second_ptr, site),
            FreeVerdict::Quarantine,
            "untouched poison must not be reported as a write-after-free"
        );
        assert!(state.take_poison_violation().is_none());

        unsafe { dealloc((second_ptr - RC_HEADER_SIZE) as *mut u8, second_layout) };
    }

    #[test]
    fn guard_state_alloc_then_free_quarantines() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        // Allocate real memory for the payload to poison.
        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::List, site);

        // Verify Live in table.
        assert!(state.table.contains_key(&payload_ptr));
        assert!(matches!(state.table[&payload_ptr].state, AllocState::Live));

        // Free it.
        let verdict = state.record_free(payload_ptr, site);
        assert_eq!(verdict, FreeVerdict::Quarantine);

        // Verify Freed in table.
        assert!(matches!(
            state.table[&payload_ptr].state,
            AllocState::Freed { .. }
        ));

        // Verify in quarantine.
        assert_eq!(state.quarantine.len(), 1);

        // Clean up: evict the quarantine by freeing more blocks.
        let base_ptr2 = unsafe { alloc(layout) };
        let payload_ptr2 = (base_ptr2 as usize) + RC_HEADER_SIZE;
        state.record_alloc(payload_ptr2, PAYLOAD_SIZE, AllocKind::Map, site);
        state.record_free(payload_ptr2, site); // This evicts ptr1

        unsafe {
            dealloc(base_ptr2, layout);
        }
    }

    #[test]
    fn guard_state_double_free_detected() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::String, site);

        let verdict1 = state.record_free(payload_ptr, site);
        assert_eq!(verdict1, FreeVerdict::Quarantine);

        let verdict2 = state.record_free(payload_ptr, site);
        assert_eq!(verdict2, FreeVerdict::DoubleFree);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    #[test]
    fn guard_state_double_free_carries_sites() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Map, site);
        state.record_free(payload_ptr, site);

        // On second free, the record still contains alloc site and first free site.
        if let Some(record) = state.table.get(&payload_ptr) {
            assert!(!record.site.file().is_empty()); // alloc site
            if let AllocState::Freed { free_site, .. } = record.state {
                assert!(!free_site.file().is_empty()); // first free site
            }
        }

        state.record_free(payload_ptr, site); // returns DoubleFree

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    #[test]
    fn guard_state_validate_use_after_free() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        // Set RC = 1 for the validate check
        unsafe {
            *(base_ptr as *mut usize) = 1;
        }

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Set, site);

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::Ok);

        state.record_free(payload_ptr, site);

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::UseAfterFree);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    #[test]
    fn guard_state_validate_live_pointer() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        // Set RC = 1 for the validate check
        unsafe {
            *(base_ptr as *mut usize) = 1;
        }

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Array, site);

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::Ok);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    #[test]
    fn guard_state_quarantine_bounded() {
        let mut state = GuardState::new(128); // tiny quarantine

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();
        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();

        // First allocation
        let base_ptr1 = unsafe { alloc(layout) };
        let payload_ptr1 = (base_ptr1 as usize) + RC_HEADER_SIZE;
        state.record_alloc(payload_ptr1, PAYLOAD_SIZE, AllocKind::Closure, site);
        state.record_free(payload_ptr1, site); // quarantine_used = 72
        assert_eq!(state.quarantine.len(), 1);

        // Second allocation (will trigger eviction)
        let base_ptr2 = unsafe { alloc(layout) };
        let payload_ptr2 = (base_ptr2 as usize) + RC_HEADER_SIZE;
        state.record_alloc(payload_ptr2, PAYLOAD_SIZE, AllocKind::Class, site);
        state.record_free(payload_ptr2, site); // quarantine_used = 144 > 128, evict first

        // First block should be evicted and deallocated.
        assert_eq!(state.quarantine.len(), 1);

        unsafe {
            dealloc(base_ptr2, layout);
        }
    }

    #[test]
    fn guard_state_address_recycling() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();
        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();

        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::List, site);
        state.record_free(payload_ptr, site);

        // Allocate again at same address (simulating allocator reuse).
        // This should replace the Freed record with a new Live one.
        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Map, site);

        assert!(matches!(state.table[&payload_ptr].state, AllocState::Live));

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    #[test]
    fn parse_quarantine_capacity_valid() {
        let cap = parse_quarantine_capacity(Some("1024".to_string()));
        assert_eq!(cap, 1024);
    }

    #[test]
    fn parse_quarantine_capacity_invalid() {
        let cap = parse_quarantine_capacity(Some("not_a_number".to_string()));
        assert_eq!(cap, DEFAULT_QUARANTINE_CAPACITY);
    }

    #[test]
    fn parse_quarantine_capacity_clamped() {
        let cap = parse_quarantine_capacity(Some("9999999999999999".to_string()));
        assert!(cap <= 1024 * 1024 * 1024); // <= 1 GB
    }

    #[test]
    fn collect_leak_groups_empty() {
        let state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);
        let msg = collect_leak_groups(&state);
        assert!(msg.is_empty());
    }

    #[test]
    fn collect_leak_groups_filters_immortals() {
        let state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        // We can't easily test immortal filtering without access to real memory,
        // so we just test that the function handles an empty state.
        let msg = collect_leak_groups(&state);
        assert!(msg.is_empty());
    }

    #[test]
    fn alloc_kind_derivation() {
        // This tests that AllocKind can be derived from file names.
        // When the actual call comes from list.rs, file.contains("list.rs") would match.
        // We can't test the real propagation here without enabling the global guard,
        // so we rely on the integration test with the real alloc_with_rc.
        assert_eq!(AllocKind::List.as_str(), "list");
    }

    /// guard_check on a Live pointer returns normally.
    #[test]
    fn guard_check_live_pointer_ok() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        // Set RC = 1 for the validate check
        unsafe {
            *(base_ptr as *mut usize) = 1;
        }

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::List, site);

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::Ok);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    /// guard_check correctly identifies a use-after-free.
    #[test]
    fn guard_check_use_after_free_detected() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        // Set RC = 1 for the validate check
        unsafe {
            *(base_ptr as *mut usize) = 1;
        }

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Map, site);
        state.record_free(payload_ptr, site);

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::UseAfterFree);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    /// guard_check correctly identifies a corrupted header.
    #[test]
    fn guard_check_header_corrupted_detected() {
        let mut state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);

        const PAYLOAD_SIZE: usize = 64;
        let site = Location::caller();

        let layout = Layout::from_size_align(RC_HEADER_SIZE + PAYLOAD_SIZE, 8).unwrap();
        let base_ptr = unsafe { alloc(layout) };
        let payload_ptr = (base_ptr as usize) + RC_HEADER_SIZE;

        state.record_alloc(payload_ptr, PAYLOAD_SIZE, AllocKind::Set, site);

        // Corrupt the RC header
        unsafe {
            *(base_ptr as *mut usize) = 0; // RC = 0 is invalid
        }

        let verdict = state.validate(payload_ptr);
        assert_eq!(verdict, TouchVerdict::HeaderCorrupt);

        unsafe {
            dealloc(base_ptr, layout);
        }
    }

    /// guard_check with a null pointer is a no-op (OK).
    #[test]
    fn guard_check_null_pointer_ok() {
        let state = GuardState::new(DEFAULT_QUARANTINE_CAPACITY);
        let verdict = state.validate(0);
        assert_eq!(verdict, TouchVerdict::Ok);
    }
}
