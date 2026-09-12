// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Cost-model telemetry for GPU residency.
//!
//! Five process-wide counters make the residency cost model observable from
//! Miri source: an
//! upload moves host bytes to a device buffer, a launch dispatches a kernel,
//! a readback copies device bytes back to the host, a fence is a
//! host-side wait on outstanding device work, and a release frees a resident
//! device buffer when its owning binding leaves scope. The persistent-buffer
//! launch path increments these so a program can assert that a multi-stage
//! pipeline pays exactly one upload + N launches + one readback and frees its
//! buffer once.
//!
//! Counters are plain atomics with no device dependency, so the accounting
//! is unit-testable without an adapter.
//!
//! The same counters travel out of the process for a run started by
//! `miri run`: when `MIRI_GPU_TELEMETRY_PATH` names a file, every change
//! rewrites it, and the compiler reads the last state into the run envelope's
//! `gpu` object. Rewriting on every change rather than at process teardown is
//! what keeps the account of a program that died mid-dispatch — a trap ends the
//! process with `_exit`, which runs no teardown hook at all.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static UPLOADS: AtomicU64 = AtomicU64::new(0);
static LAUNCHES: AtomicU64 = AtomicU64::new(0);
static READBACKS: AtomicU64 = AtomicU64::new(0);
static FENCES: AtomicU64 = AtomicU64::new(0);
static RELEASES: AtomicU64 = AtomicU64::new(0);

pub fn record_upload() {
    UPLOADS.fetch_add(1, Ordering::SeqCst);
    report();
}

pub fn record_launch() {
    LAUNCHES.fetch_add(1, Ordering::SeqCst);
    report();
}

pub fn record_readback() {
    READBACKS.fetch_add(1, Ordering::SeqCst);
    report();
}

pub fn record_fence() {
    FENCES.fetch_add(1, Ordering::SeqCst);
    report();
}

pub fn record_release() {
    RELEASES.fetch_add(1, Ordering::SeqCst);
    report();
}

pub fn reset() {
    UPLOADS.store(0, Ordering::SeqCst);
    LAUNCHES.store(0, Ordering::SeqCst);
    READBACKS.store(0, Ordering::SeqCst);
    FENCES.store(0, Ordering::SeqCst);
    RELEASES.store(0, Ordering::SeqCst);
    report();
}

/// Where this process reports its counters, when it was started by a command
/// that asked for them. Read once: the environment belongs to the spawning
/// command and does not change under a running program.
fn report_path() -> Option<&'static PathBuf> {
    static PATH: OnceLock<Option<PathBuf>> = OnceLock::new();
    PATH.get_or_init(|| std::env::var_os(REPORT_PATH_VAR).map(PathBuf::from))
        .as_ref()
}

/// Environment variable naming the file the counters are reported to.
const REPORT_PATH_VAR: &str = "MIRI_GPU_TELEMETRY_PATH";

/// Write the current counters where the spawning command can read them.
///
/// Best-effort in the same sense as the trap report: nothing the program does
/// depends on the write landing, and a run started without the variable
/// reports nothing. The record is `name=count` pairs so a reader that does not
/// know a counter can ignore it rather than mis-read a position.
pub fn report() {
    let Some(path) = report_path() else {
        return;
    };
    let _ = std::fs::write(path, snapshot());
}

/// The counters as the `name=count` text a report carries.
fn snapshot() -> String {
    format!(
        "uploads={} launches={} readbacks={} fences={} releases={}",
        UPLOADS.load(Ordering::SeqCst),
        LAUNCHES.load(Ordering::SeqCst),
        READBACKS.load(Ordering::SeqCst),
        FENCES.load(Ordering::SeqCst),
        RELEASES.load(Ordering::SeqCst),
    )
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_reset() {
    reset();
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_uploads() -> u64 {
    UPLOADS.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_launches() -> u64 {
    LAUNCHES.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_readbacks() -> u64 {
    READBACKS.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_fences() -> u64 {
    FENCES.load(Ordering::SeqCst)
}

#[no_mangle]
pub extern "C" fn miri_gpu_telemetry_releases() -> u64 {
    RELEASES.load(Ordering::SeqCst)
}

#[cfg(test)]
mod tests {
    use super::snapshot;

    /// The record's shape is a contract with the compiler that reads it back:
    /// `name=count` pairs, one per counter, so a reader that does not know a
    /// counter ignores it instead of mis-reading a position.
    #[test]
    fn the_record_names_every_counter_it_carries() {
        let record = snapshot();
        for counter in ["uploads", "launches", "readbacks", "fences", "releases"] {
            assert!(
                record
                    .split_whitespace()
                    .filter_map(|field| field.split_once('='))
                    .any(|(key, value)| key == counter && value.parse::<u64>().is_ok()),
                "`{counter}` is missing a count in `{record}`"
            );
        }
    }
}
