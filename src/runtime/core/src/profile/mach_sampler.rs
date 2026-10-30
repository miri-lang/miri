// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The macOS sampler: a thread of its own that, each time the program's main
//! thread has spent another period of CPU time, suspends it, reads its
//! registers, walks its frame-pointer chain while it stands still, and
//! resumes it.
//!
//! macOS has no timer that signals one thread after it has used a span of
//! CPU time, and the process-wide `ITIMER_PROF` raises far fewer signals than
//! asked for — a fifth to a half of the rate on a busy loop. The sampler
//! thread instead wakes on a short wall-clock cadence, reads how much CPU
//! time the main thread has used since it last looked, and takes a sample
//! once that reaches the sampling period, so a program that waits uses no
//! samples. It asks the scheduler for a time-constraint policy so that it
//! wakes on time; if the kernel refuses, it runs at its normal priority.
//!
//! While the main thread is suspended the sampler only makes Mach calls,
//! reads the main thread's stack and appends to the preallocated sample
//! buffer: it allocates nothing and takes no lock, so it cannot wait on one
//! the suspended thread holds.

use super::frame_walk::Registers;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A Mach port name, `mach_port_t`.
type MachPort = libc::c_uint;

/// A Mach call's result, `kern_return_t`.
type KernReturn = libc::c_int;

/// Mach's unsigned machine word, `natural_t`.
type Natural = libc::c_uint;

/// The result of a Mach call that succeeded.
const KERN_SUCCESS: KernReturn = 0;

/// The `thread_info` flavor of [`ThreadBasicInfo`].
const THREAD_BASIC_INFO: Natural = 3;

/// The `thread_policy_set` flavor of [`TimeConstraintPolicy`].
const THREAD_TIME_CONSTRAINT_POLICY: Natural = 2;

/// The longest the sampler sleeps between looks at the main thread's CPU
/// time, whatever the rate, so that stopping it never waits long.
const LONGEST_WAKE: Duration = Duration::from_millis(10);

/// The longest `stop` waits for the sampler thread to finish its last look.
const STOP_TIMEOUT: Duration = Duration::from_secs(1);

/// The CPU time the sampler expects of itself each time it wakes, and the
/// least the kernel grants a time-constraint thread.
const WAKE_COMPUTATION: Duration = Duration::from_micros(50);

/// `time_value_t`: a span of time in seconds and microseconds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct TimeValue {
    seconds: libc::c_int,
    microseconds: libc::c_int,
}

impl TimeValue {
    /// The span in nanoseconds; a negative field counts as none.
    fn nanoseconds(self) -> u64 {
        let seconds = u64::try_from(self.seconds).unwrap_or(0);
        let microseconds = u64::try_from(self.microseconds).unwrap_or(0);
        seconds * 1_000_000_000 + microseconds * 1_000
    }
}

/// `thread_basic_info`, as `thread_info` fills it for [`THREAD_BASIC_INFO`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct ThreadBasicInfo {
    user_time: TimeValue,
    system_time: TimeValue,
    cpu_usage: libc::c_int,
    policy: libc::c_int,
    run_state: libc::c_int,
    flags: libc::c_int,
    suspend_count: libc::c_int,
    sleep_time: libc::c_int,
}

/// `thread_time_constraint_policy`, its spans in Mach absolute time units.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct TimeConstraintPolicy {
    period: u32,
    computation: u32,
    constraint: u32,
    preemptible: libc::c_int,
}

/// `mach_timebase_info_data_t`: Mach absolute time units are
/// `numer / denom` nanoseconds.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct Timebase {
    numer: u32,
    denom: u32,
}

/// `arm_thread_state64_t`, the `ARM_THREAD_STATE64` flavor.
#[cfg(target_arch = "aarch64")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct ThreadState {
    x: [u64; 29],
    fp: u64,
    lr: u64,
    sp: u64,
    pc: u64,
    cpsr: u32,
    flags: u32,
}

/// The `thread_get_state` flavor of [`ThreadState`].
#[cfg(target_arch = "aarch64")]
const THREAD_STATE_FLAVOR: libc::c_int = 6;

#[cfg(target_arch = "aarch64")]
impl ThreadState {
    fn registers(&self) -> Registers {
        Registers {
            pc: self.pc as usize,
            fp: self.fp as usize,
            sp: self.sp as usize,
            lr: self.lr as usize,
        }
    }
}

/// `x86_thread_state64_t`, the `x86_THREAD_STATE64` flavor.
#[cfg(target_arch = "x86_64")]
#[repr(C)]
#[derive(Debug, Clone, Copy, Default)]
struct ThreadState {
    rax: u64,
    rbx: u64,
    rcx: u64,
    rdx: u64,
    rdi: u64,
    rsi: u64,
    rbp: u64,
    rsp: u64,
    r: [u64; 8],
    rip: u64,
    rflags: u64,
    cs: u64,
    fs: u64,
    gs: u64,
}

/// The `thread_get_state` flavor of [`ThreadState`].
#[cfg(target_arch = "x86_64")]
const THREAD_STATE_FLAVOR: libc::c_int = 4;

#[cfg(target_arch = "x86_64")]
impl ThreadState {
    fn registers(&self) -> Registers {
        Registers {
            pc: self.rip as usize,
            fp: self.rbp as usize,
            sp: self.rsp as usize,
            lr: 0,
        }
    }
}

/// The size of `T` in the `natural_t` words a Mach call counts it in.
const fn words_of<T>() -> Natural {
    (std::mem::size_of::<T>() / std::mem::size_of::<Natural>()) as Natural
}

// The structures above have the sizes the kernel's own headers give them.
const _: () = assert!(words_of::<ThreadBasicInfo>() == 10);
const _: () = assert!(words_of::<TimeConstraintPolicy>() == 4);
#[cfg(target_arch = "aarch64")]
const _: () = assert!(words_of::<ThreadState>() == 68);
#[cfg(target_arch = "x86_64")]
const _: () = assert!(words_of::<ThreadState>() == 42);

extern "C" {
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> KernReturn;
    fn mach_wait_until(deadline: u64) -> KernReturn;
    fn thread_suspend(target: MachPort) -> KernReturn;
    fn thread_resume(target: MachPort) -> KernReturn;
    fn thread_get_state(
        target: MachPort,
        flavor: libc::c_int,
        state: *mut Natural,
        count: *mut Natural,
    ) -> KernReturn;
    fn thread_info(
        target: MachPort,
        flavor: Natural,
        info: *mut libc::c_int,
        count: *mut Natural,
    ) -> KernReturn;
    fn thread_policy_set(
        thread: MachPort,
        flavor: Natural,
        policy: *mut libc::c_int,
        count: Natural,
    ) -> KernReturn;
}

/// The most periods of CPU time the sampler carries unsampled. The kernel
/// advances a running thread's CPU time in steps, so one look can find
/// several periods used and the next none; one sample is taken per wake, so
/// what a step leaves owed is taken over the wakes that follow.
const OWED_PERIODS: u64 = 4;

/// Set to ask the sampler thread to finish.
static STOP: AtomicBool = AtomicBool::new(false);

/// Set while the sampler thread runs.
static RUNNING: AtomicBool = AtomicBool::new(false);

/// How much CPU time the sampled thread has used that no sample has stood
/// for yet. A sample is owed each time it reaches a period; what one look
/// leaves owed is carried to the next, up to [`OWED_PERIODS`], so a sampler
/// that falls behind catches up a sample per wake rather than in a burst,
/// and a thread that stops computing owes only a few samples more.
#[derive(Debug, Clone, Copy)]
struct CpuDebt {
    period: u64,
    owed: u64,
    last_seen: u64,
}

impl CpuDebt {
    fn new(period: u64, cpu_now: u64) -> Self {
        Self {
            period: period.max(1),
            owed: 0,
            last_seen: cpu_now,
        }
    }

    /// Charge the CPU time used up to `cpu_now`; returns whether a sample is
    /// owed, and if so, takes one period off the debt.
    fn is_sample_due(&mut self, cpu_now: u64) -> bool {
        let used = cpu_now.saturating_sub(self.last_seen);
        self.last_seen = self.last_seen.max(cpu_now);
        self.owed = self
            .owed
            .saturating_add(used)
            .min(OWED_PERIODS * self.period);
        let is_due = self.owed >= self.period;
        if is_due {
            self.owed -= self.period;
        }
        is_due
    }
}

/// Conversion between nanoseconds and Mach absolute time units.
#[derive(Debug, Clone, Copy)]
struct Clock {
    timebase: Timebase,
}

impl Clock {
    fn new() -> Option<Self> {
        let mut timebase = Timebase::default();
        // SAFETY: `timebase` is a valid `mach_timebase_info_data_t` for the
        // call to write.
        let read = unsafe { mach_timebase_info(&mut timebase) };
        (read == KERN_SUCCESS && timebase.numer != 0 && timebase.denom != 0)
            .then_some(Self { timebase })
    }

    /// `span` in absolute time units, saturating.
    fn ticks(self, span: Duration) -> u64 {
        let ticks =
            span.as_nanos() * u128::from(self.timebase.denom) / u128::from(self.timebase.numer);
        u64::try_from(ticks).unwrap_or(u64::MAX)
    }

    fn now() -> u64 {
        // SAFETY: `mach_absolute_time` has no preconditions.
        unsafe { mach_absolute_time() }
    }
}

/// The CPU time, user and system, `thread` has used, in nanoseconds; `None`
/// once the thread is gone.
fn cpu_time_of(thread: MachPort) -> Option<u64> {
    let mut info = ThreadBasicInfo::default();
    let mut count = words_of::<ThreadBasicInfo>();
    // SAFETY: `info` is a `thread_basic_info` of `count` words for the call
    // to fill, and `count` is valid for it to update.
    let read = unsafe {
        thread_info(
            thread,
            THREAD_BASIC_INFO,
            std::ptr::addr_of_mut!(info).cast(),
            &mut count,
        )
    };
    (read == KERN_SUCCESS).then(|| info.user_time.nanoseconds() + info.system_time.nanoseconds())
}

/// The registers of the suspended `thread`.
fn registers_of(thread: MachPort) -> Option<Registers> {
    let mut state = ThreadState::default();
    let mut count = words_of::<ThreadState>();
    // SAFETY: `state` is the structure of `THREAD_STATE_FLAVOR`, `count`
    // words long, for the call to fill; `count` is valid for it to update.
    let read = unsafe {
        thread_get_state(
            thread,
            THREAD_STATE_FLAVOR,
            std::ptr::addr_of_mut!(state).cast(),
            &mut count,
        )
    };
    (read == KERN_SUCCESS).then(|| state.registers())
}

/// Suspend `thread`, record a sample of it, and resume it. Between the two
/// it allocates nothing and takes no lock. Returns whether the thread could
/// be suspended and resumed.
fn sample(thread: MachPort) -> bool {
    // SAFETY: suspending a thread of this process touches none of its
    // memory; the thread is resumed below on every path.
    if unsafe { thread_suspend(thread) } != KERN_SUCCESS {
        return false;
    }
    if let Some(registers) = registers_of(thread) {
        super::record_sample(registers, true);
    }
    // SAFETY: `thread` was suspended above, once, by this thread.
    unsafe { thread_resume(thread) == KERN_SUCCESS }
}

/// Ask the scheduler to wake the calling thread every `period` ticks,
/// promptly. A refusal leaves it at its normal priority.
fn raise_priority(clock: Clock, period: u64) {
    let computation = clock.ticks(WAKE_COMPUTATION).min(period);
    let mut policy = TimeConstraintPolicy {
        period: u32::try_from(period).unwrap_or(u32::MAX),
        computation: u32::try_from(computation).unwrap_or(u32::MAX),
        constraint: u32::try_from(period).unwrap_or(u32::MAX),
        preemptible: 1,
    };
    // SAFETY: `pthread_self` has no preconditions, and the port it maps to
    // names the calling thread without adding a reference; `policy` is a
    // `thread_time_constraint_policy` of the count passed.
    unsafe {
        let thread = libc::pthread_mach_thread_np(libc::pthread_self());
        thread_policy_set(
            thread,
            THREAD_TIME_CONSTRAINT_POLICY,
            std::ptr::addr_of_mut!(policy).cast(),
            words_of::<TimeConstraintPolicy>(),
        );
    }
}

/// The sampler thread's body: sample `thread` once per `period` of the CPU
/// time it uses, until asked to stop or the thread is gone.
fn run(thread: MachPort, period: Duration, clock: Clock) {
    let wake = clock.ticks(period.min(LONGEST_WAKE)).max(1);
    raise_priority(clock, wake);
    let period_ns = u64::try_from(period.as_nanos()).unwrap_or(u64::MAX);
    let Some(cpu_now) = cpu_time_of(thread) else {
        return;
    };
    let mut debt = CpuDebt::new(period_ns, cpu_now);
    let mut deadline = Clock::now().saturating_add(wake);
    while !STOP.load(Ordering::Acquire) {
        // SAFETY: `mach_wait_until` only blocks the calling thread.
        unsafe { mach_wait_until(deadline) };
        deadline = deadline.saturating_add(wake).max(Clock::now());
        let Some(cpu_now) = cpu_time_of(thread) else {
            return;
        };
        if debt.is_sample_due(cpu_now) && !sample(thread) {
            return;
        }
    }
}

/// Start sampling the calling thread, the program's main thread, `hz` times
/// per second of the CPU time it uses. Returns whether the sampler thread
/// started.
pub(crate) fn start(hz: u32) -> bool {
    let Some(clock) = Clock::new() else {
        return false;
    };
    // SAFETY: `pthread_self` has no preconditions; the port names the
    // calling thread for as long as it lives, without adding a reference.
    let thread = unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) };
    let period = Duration::from_secs(1) / hz.max(1);
    RUNNING.store(true, Ordering::Release);
    let spawned = std::thread::Builder::new()
        .name("miri-profiler".to_string())
        .stack_size(64 * 1024)
        .spawn(move || {
            run(thread, period, clock);
            RUNNING.store(false, Ordering::Release);
        });
    if spawned.is_err() {
        RUNNING.store(false, Ordering::Release);
    }
    spawned.is_ok()
}

/// Stop the sampler thread and wait, briefly, for it to finish the sample it
/// may be taking, so none is written while the record is read.
pub(crate) fn stop() {
    STOP.store(true, Ordering::Release);
    let started = Instant::now();
    while RUNNING.load(Ordering::Acquire) && started.elapsed() < STOP_TIMEOUT {
        std::thread::sleep(Duration::from_micros(100));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MS: u64 = 1_000_000;

    #[test]
    fn a_sample_is_due_once_a_period_of_cpu_time_is_used() {
        let mut debt = CpuDebt::new(MS, 5 * MS);
        assert!(!debt.is_sample_due(5 * MS + MS / 2));
        assert!(debt.is_sample_due(6 * MS));
        assert!(!debt.is_sample_due(6 * MS));
    }

    #[test]
    fn a_waiting_thread_owes_no_sample() {
        let mut debt = CpuDebt::new(MS, 0);
        for _ in 0..100 {
            assert!(!debt.is_sample_due(MS / 2));
        }
    }

    #[test]
    fn a_late_wake_carries_a_bounded_debt() {
        let mut debt = CpuDebt::new(MS, 0);
        let due = (0..10).filter(|_| debt.is_sample_due(10 * MS)).count();
        assert_eq!(due as u64, OWED_PERIODS, "one sample a wake, then no more");
    }

    #[test]
    fn the_clock_converts_a_millisecond_to_ticks_and_back() {
        let Some(clock) = Clock::new() else {
            panic!("no timebase");
        };
        let ticks = clock.ticks(Duration::from_millis(1));
        let nanoseconds =
            u128::from(ticks) * u128::from(clock.timebase.numer) / u128::from(clock.timebase.denom);
        assert!(
            (999_000..=1_000_000).contains(&nanoseconds),
            "{nanoseconds}"
        );
    }

    #[test]
    fn the_calling_threads_cpu_time_grows_as_it_computes() {
        // SAFETY: `pthread_self` has no preconditions.
        let thread = unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) };
        let Some(before) = cpu_time_of(thread) else {
            panic!("no CPU time before");
        };
        let started = Instant::now();
        let mut spin = 0u64;
        while started.elapsed() < Duration::from_millis(20) {
            spin = std::hint::black_box(spin.wrapping_add(1));
        }
        let Some(after) = cpu_time_of(thread) else {
            panic!("no CPU time after");
        };
        assert!(after - before >= 10 * MS, "{before} then {after}");
    }
}
