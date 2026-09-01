// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri dev` command: re-check a file whenever it or a neighbour changes.
//!
//! The session checks once at startup, so a tool that attaches learns the
//! current state without having to touch a file first, and then once per change
//! — never on a timer. Nothing is reported by a poll that found nothing, which
//! is what keeps a quiet session quiet.
//!
//! Under `--format json` stdout carries stream lines and nothing else: a stray
//! line written among them would be read as a malformed object and cost the
//! consumer the stream. Everything meant for a person goes to stderr.

use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant, SystemTime};

use crate::cli::{check, ColorMode, Format};
use crate::diagnostics::jsonl::{self, DevEvent};

/// How long to wait between two looks at the watched directory.
///
/// Short enough that a save feels answered, long enough that an idle session
/// costs nothing measurable.
const POLL_INTERVAL_MS: u64 = 250;

/// How the session ended.
pub enum Outcome {
    /// The session ended because its consumer went away.
    Exited,
    /// The session could not start, or could not go on.
    Failed,
}

/// The modification time of every file a change would matter to.
type Mtimes = HashMap<PathBuf, SystemTime>;

/// Whether the session can go on after reporting one check.
///
/// A watch session ends for exactly one reason of its own: the consumer of its
/// stream went away. Everything else it survives.
enum Reported {
    /// The report reached its consumer.
    Delivered,
    /// The stream can no longer be written. The session is over.
    ConsumerGone,
}

/// Watch `path` and re-check it whenever it or a sibling changes.
///
/// The loop has no exit of its own: a watch session runs until it is
/// interrupted, or until the consumer of its stream closes the pipe.
pub fn run(path: PathBuf, format: Format, verify_mir: bool, color_mode: ColorMode) -> Outcome {
    let watched = match path.canonicalize() {
        Ok(watched) => watched,
        Err(error) => {
            eprintln!("error: could not read {}: {}", path.display(), error);
            return Outcome::Failed;
        }
    };

    let Some(directory) = watched.parent().map(Path::to_path_buf) else {
        eprintln!("error: {} has no parent directory", path.display());
        return Outcome::Failed;
    };

    let mut mtimes = match gather_mtimes(&directory) {
        Ok(mtimes) => mtimes,
        Err(error) => {
            eprintln!("error: could not read {}: {}", directory.display(), error);
            return Outcome::Failed;
        }
    };

    let session_start = Instant::now();
    if let Reported::ConsumerGone =
        check_once(&watched, &session_start, format, verify_mir, color_mode)
    {
        return Outcome::Exited;
    }

    loop {
        std::thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));

        let current = match gather_mtimes(&directory) {
            Ok(current) => current,
            Err(error) => {
                eprintln!("error: could not read {}: {}", directory.display(), error);
                return Outcome::Failed;
            }
        };

        if current == mtimes {
            continue;
        }
        mtimes = current;

        if let Reported::ConsumerGone =
            check_once(&watched, &session_start, format, verify_mir, color_mode)
        {
            return Outcome::Exited;
        }
    }
}

/// Check `watched` once and report what it found.
///
/// Only a stream that can no longer be written ends the session. A file that
/// cannot be read is reported and the session goes on, because an editor that
/// saves by replacing a file leaves a moment where the path does not resolve,
/// and a session that died there would be useless.
fn check_once(
    watched: &Path,
    session_start: &Instant,
    format: Format,
    verify_mir: bool,
    color_mode: ColorMode,
) -> Reported {
    match report_once(watched, session_start, format, verify_mir, color_mode) {
        Ok(()) => Reported::Delivered,
        Err(_) => Reported::ConsumerGone,
    }
}

/// Check `watched` once and write the report.
///
/// The error case is narrow on purpose: it means a write failed, which is how a
/// consumer that closed the pipe makes itself known. Nothing else here reports
/// through the return value.
fn report_once(
    watched: &Path,
    session_start: &Instant,
    format: Format,
    verify_mir: bool,
    color_mode: ColorMode,
) -> std::io::Result<()> {
    let ts = session_start.elapsed().as_millis() as u64;

    // A watch session reports itself as a stream of events, so a file that
    // cannot be read is written for a person rather than folded into an
    // envelope the stream does not carry. The diagnostic is the shared one, so
    // the code and the help line are the ones every other command shows.
    let source = match crate::cli::source::read(watched) {
        Ok(source) => source,
        Err(diagnostic) => {
            eprint!(
                "{}",
                crate::error::format::format_diagnostic_with_color(
                    "",
                    &diagnostic,
                    Some(&watched.display().to_string()),
                    color_mode.into(),
                )
            );
            return Ok(());
        }
    };

    // The batch opens before the check runs, so a consumer learns that work
    // started rather than hearing nothing until it finished.
    if format == Format::Json {
        let opening = DevEvent::tick(ts, watched.display().to_string());
        jsonl::write_event(&mut std::io::stdout().lock(), &opening)?;
    }

    let started = Instant::now();
    let report = check::check(watched, &source, verify_mir);
    let duration_ms = started.elapsed().as_millis() as u64;

    match format {
        Format::Json => write_stream_batch(&report, duration_ms),
        Format::Pretty => write_for_a_person(&report, &source, color_mode),
    }
}

/// Write the diagnostics and the closing line of one batch.
///
/// The stream is locked once for the whole tail rather than once per line: the
/// lines of a batch belong together, and nothing else writes to this stream.
fn write_stream_batch(report: &check::CheckReport, duration_ms: u64) -> std::io::Result<()> {
    let mut stdout = std::io::stdout().lock();
    for diagnostic in &report.envelope.diagnostics {
        jsonl::write_diagnostic(&mut stdout, diagnostic)?;
    }
    jsonl::write_event(&mut stdout, &DevEvent::idle(report.ok, duration_ms))
}

/// Render one check the way `miri check` renders it.
///
/// Diagnostics go to stderr and the closing summary to stdout, which is the
/// split the single-shot command already uses; a session should not report the
/// same check two different ways.
fn write_for_a_person(
    report: &check::CheckReport,
    source: &str,
    color_mode: ColorMode,
) -> std::io::Result<()> {
    for rendered in report.rendered_diagnostics(source, color_mode) {
        eprintln!("{}", rendered);
    }
    match report.summary() {
        Some(summary) => writeln!(std::io::stdout(), "{}", summary),
        None => Ok(()),
    }
}

/// The modification time of every `.mi` file in `directory`.
///
/// A module resolves its imports against the directory it sits in, so a change
/// to a neighbour can change what the watched file means. Unreadable entries are
/// left out rather than failing the sweep: a file being written at the moment it
/// is stat'd reappears on the next poll, and its absence from one map is itself
/// a difference the loop will act on.
fn gather_mtimes(directory: &Path) -> std::io::Result<Mtimes> {
    let mut mtimes = Mtimes::new();
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.extension().and_then(|extension| extension.to_str()) != Some("mi") {
            continue;
        }
        if let Ok(mtime) = fs::metadata(&path).and_then(|metadata| metadata.modified()) {
            mtimes.insert(path, mtime);
        }
    }
    Ok(mtimes)
}
