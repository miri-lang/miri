// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The sampling profiler a `--profile` build carries.
//!
//! A profile build's entry point hands [`ffi::miri_rt_profile_init`] the
//! table of its compiled functions. When `MIRI_PROFILE_OUT` names a file, the
//! runtime arms a CPU-time timer whose `SIGPROF` handler records the
//! interrupted instruction and, on the main thread, the frame-pointer chain
//! above it; at exit it writes the samples to that file (see [`record`]).
//! Without the variable nothing is installed and the program runs as a
//! release build does. `MIRI_PROFILE_HZ` sets the rate, 1000 by default.
//!
//! A sample on a thread other than the main one records its instruction
//! alone: only the main thread's stack bounds are known, and they are read
//! once, at start-up, because the calls that read them are not safe in a
//! signal handler.

mod frame_walk;
mod function_table;
mod record;
mod sample_buffer;
mod sampler;

use frame_walk::{LeafCode, Registers, Sample, StackBounds, MAX_FRAMES};
use function_table::FunctionTable;
pub use function_table::ProfileFunction;
use record::{RunSummary, StackCounts};
use sample_buffer::SampleBuffer;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// The variable naming the file a profiled run writes its record to.
const OUT_VARIABLE: &str = "MIRI_PROFILE_OUT";

/// The variable setting how many samples a second of CPU time takes.
const RATE_VARIABLE: &str = "MIRI_PROFILE_HZ";

/// The rate a run samples at when `MIRI_PROFILE_HZ` does not set one.
const DEFAULT_HZ: u32 = 1000;

/// The highest rate `MIRI_PROFILE_HZ` may ask for.
const MAX_HZ: u32 = 10_000;

/// The words of the sample buffer: 32 MiB, about four hundred thousand
/// samples of a ten-frame stack, taken from the allocator as untouched pages.
const BUFFER_WORDS: usize = 1 << 22;

/// A run being profiled.
struct Profiler {
    table: FunctionTable,
    buffer: SampleBuffer,
    /// The thread the entry point ran on, whose stack a sample walks.
    main_thread: usize,
    main_stack: Option<StackBounds>,
    out_path: PathBuf,
    hz: u32,
}

/// The run being profiled, set once before the first sample.
static PROFILER: OnceLock<Profiler> = OnceLock::new();

/// Set once the record has been written, so the exit paths write it once.
static RECORD_WRITTEN: AtomicBool = AtomicBool::new(false);

/// The rate `requested` asks for: a whole number from 1 to [`MAX_HZ`], or the
/// default when nothing is asked. `Err` carries the text that is not a rate.
fn sampling_rate(requested: Option<&str>) -> Result<u32, &str> {
    let Some(text) = requested else {
        return Ok(DEFAULT_HZ);
    };
    match text.trim().parse::<u32>() {
        Ok(hz) if (1..=MAX_HZ).contains(&hz) => Ok(hz),
        Ok(_) | Err(_) => Err(text),
    }
}

/// Record one sample from the interrupted `registers`. Runs in the signal
/// handler: it allocates nothing, takes no lock, and reads only the stack
/// its bounds checks admit.
fn record_sample(registers: Registers) {
    let Some(profiler) = PROFILER.get() else {
        return;
    };
    let mut frames = [0usize; MAX_FRAMES];
    let main_stack = profiler
        .main_stack
        .filter(|stack| stack.contains(registers.sp))
        .filter(|_| sampler::current_thread() == profiler.main_thread);
    let leaf = profiler
        .table
        .code_of(registers.pc)
        .map(|(start, size)| LeafCode {
            // SAFETY: the table lists the build's own compiled functions, each
            // `size` bytes of code from `start`, mapped readable for the life of
            // the program; the table is not written after it is built.
            bytes: unsafe { std::slice::from_raw_parts(start as *const u8, size) },
            offset: registers.pc - start,
        });
    let count = match main_stack {
        Some(stack) => frame_walk::walk_frames(
            Sample { registers, leaf },
            stack,
            // SAFETY: `walk_frames` asks only for aligned words inside the
            // interrupted thread's own stack, at or above its stack pointer,
            // which the handler is running on and which stays mapped.
            |address| unsafe { *(address as *const usize) },
            &mut frames,
        ),
        None => {
            frames[0] = frame_walk::code_address(registers.pc);
            1
        }
    };
    profiler.buffer.push(&frames[..count]);
}

/// Write the record once, when the program exits: stop the timer, ignore any
/// sample it already raised, and write what the buffer holds. Called by the
/// `atexit` hook and by the runtime's `exit`, which skips `atexit`.
pub(crate) fn finish() {
    let Some(profiler) = PROFILER.get() else {
        return;
    };
    if RECORD_WRITTEN.swap(true, Ordering::AcqRel) {
        return;
    }
    sampler::stop_timer();
    sampler::ignore_signal();
    let mut stacks = StackCounts::default();
    let incomplete = profiler
        .buffer
        .for_each_sample(|frames| stacks.add(frames, &profiler.table));
    let summary = RunSummary {
        hz: profiler.hz,
        dropped: profiler.buffer.dropped() + incomplete,
    };
    let text = record::render(&stacks, summary, sampler::symbol_at);
    if let Err(error) = write_record(&profiler.out_path, &text) {
        let _ = writeln!(
            std::io::stderr(),
            "miri: could not write the profile to {}: {error}",
            profiler.out_path.display()
        );
    }
}

/// Write `text` to `path` as a new file only its owner can read, replacing
/// one already there, and refusing to follow a symlink in its place: the
/// path comes from the environment, and a link planted at it must not turn
/// the write into one somewhere else.
fn write_record(path: &std::path::Path, text: &str) -> std::io::Result<()> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    // `mode` applies to a file this call creates; one already there keeps
    // its own until it is set.
    file.set_permissions(std::fs::Permissions::from_mode(0o600))?;
    file.write_all(text.as_bytes())
}

/// The `atexit` hook that writes the record.
extern "C" fn finish_at_exit() {
    finish();
}

/// Say on stderr why a run asked to be profiled is not.
fn report_not_profiled(reason: &str) {
    let _ = writeln!(std::io::stderr(), "miri: {OUT_VARIABLE} ignored: {reason}");
}

/// Start profiling into `out_path` with the build's `functions`.
fn start(out_path: PathBuf, functions: &[ProfileFunction]) {
    let rate = std::env::var(RATE_VARIABLE).ok();
    let hz = sampling_rate(rate.as_deref()).unwrap_or_else(|text| {
        let _ = writeln!(
            std::io::stderr(),
            "miri: {RATE_VARIABLE}={text} is not a rate from 1 to {MAX_HZ}; sampling at {DEFAULT_HZ}"
        );
        DEFAULT_HZ
    });
    let profiler = Profiler {
        table: FunctionTable::new(functions),
        buffer: SampleBuffer::with_capacity(BUFFER_WORDS),
        main_thread: sampler::current_thread(),
        main_stack: sampler::current_stack(),
        out_path,
        hz,
    };
    if PROFILER.set(profiler).is_err() {
        return;
    }
    // SAFETY: `finish_at_exit` is an `extern "C"` function with no
    // arguments, as `atexit` requires.
    let registered = unsafe { libc::atexit(finish_at_exit) } == 0;
    if !registered || !sampler::install_handler() {
        report_not_profiled("the profiling signal handler could not be installed");
        return;
    }
    if !sampler::start_timer(hz) {
        report_not_profiled("the profiling timer could not be started");
    }
}

pub mod ffi {
    use super::*;

    /// Called once, first thing, by the entry point of a profile build, with
    /// the table of its compiled functions in the order of the build's line
    /// map. Starts sampling when `MIRI_PROFILE_OUT` names a file; otherwise
    /// installs nothing.
    ///
    /// # Safety
    /// `functions` must point to `count` readable [`ProfileFunction`]s, or be
    /// null with a `count` of zero. The table is read here and not kept.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_profile_init(functions: *const ProfileFunction, count: usize) {
        let Some(out_path) = std::env::var_os(OUT_VARIABLE).filter(|path| !path.is_empty()) else {
            return;
        };
        if !sampler::IS_SUPPORTED {
            report_not_profiled("this target has no sampling profiler");
            return;
        }
        let functions = if functions.is_null() || count == 0 {
            &[][..]
        } else {
            // SAFETY: the caller hands over `count` readable entries.
            unsafe { std::slice::from_raw_parts(functions, count) }
        };
        start(PathBuf::from(out_path), functions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    /// A directory of its own under the system's temporary directory.
    fn scratch(name: &str) -> PathBuf {
        let directory =
            std::env::temp_dir().join(format!("miri-profile-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&directory);
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn the_record_is_written_readable_by_its_owner_alone() {
        let directory = scratch("mode");
        let path = directory.join("record");
        std::fs::write(&path, "an older and longer record").unwrap();
        write_record(&path, "miri-profile 1\n").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "miri-profile 1\n");
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        let _ = std::fs::remove_dir_all(&directory);
        assert_eq!(mode & 0o777, 0o600, "{mode:o}");
    }

    #[test]
    fn a_symlink_at_the_record_path_is_not_followed() {
        let directory = scratch("symlink");
        let target = directory.join("target");
        std::fs::write(&target, "untouched").unwrap();
        let link = directory.join("record");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let written = write_record(&link, "miri-profile 1\n");
        let kept = std::fs::read_to_string(&target).unwrap();
        let _ = std::fs::remove_dir_all(&directory);
        assert!(written.is_err(), "the record was written through a symlink");
        assert_eq!(kept, "untouched");
    }

    #[test]
    fn no_rate_asked_for_samples_at_the_default() {
        assert_eq!(sampling_rate(None), Ok(DEFAULT_HZ));
    }

    #[test]
    fn a_rate_in_range_is_taken_and_one_outside_it_is_refused() {
        assert_eq!(sampling_rate(Some("250")), Ok(250));
        assert_eq!(sampling_rate(Some(" 10000 ")), Ok(MAX_HZ));
        assert_eq!(sampling_rate(Some("0")), Err("0"));
        assert_eq!(sampling_rate(Some("10001")), Err("10001"));
        assert_eq!(sampling_rate(Some("fast")), Err("fast"));
    }
}
