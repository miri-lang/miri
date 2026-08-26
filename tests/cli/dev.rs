// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for `miri dev`, the watch session.
//!
//! Each test drives a real compiler process and reads its stream the way a tool
//! would, so what is exercised is the session as shipped rather than the
//! functions behind it.
//!
//! Nothing here asserts how *fast* a batch arrives. The suite runs its tests in
//! parallel, so a wall-clock bound would measure how busy the machine is; what
//! is asserted is that the batch arrives at all, within a bound generous enough
//! to be about correctness.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::time::Duration;

use miri::diagnostics::jsonl::{DevEvent, DevStreamLine};
use tempfile::TempDir;

/// How long a test waits for a batch before calling it lost.
///
/// Generous on purpose: it is the bound that separates "never arrived" from
/// "arrived slowly on a loaded machine", not a latency budget.
const PATIENCE: Duration = Duration::from_secs(30);

/// A live `miri dev` process a test can read the stream of.
///
/// The child is killed when this is dropped, so a failing assertion cannot
/// leave a watch session running for the rest of the suite.
struct Session {
    process: Child,
    lines: Receiver<String>,
}

impl Session {
    /// Start a session watching `path`, streaming in `format`.
    fn start(path: &Path, format: &str) -> Self {
        let mut process = Command::new(assert_cmd::cargo_bin!("miri"))
            .arg("dev")
            .arg(path)
            .arg("--format")
            .arg(format)
            .env(
                "MIRI_STDLIB_PATH",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib"),
            )
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiler binary should start");

        let stdout = process.stdout.take().expect("stdout was piped");
        Self {
            process,
            lines: reader_thread(stdout),
        }
    }

    /// Take the next line of the stream, or say what was missing.
    fn next_line(&mut self, expectation: &str) -> String {
        match self.lines.recv_timeout(PATIENCE) {
            Ok(line) => line,
            Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for {}", expectation),
            Err(RecvTimeoutError::Disconnected) => {
                panic!("the session ended before {}", expectation)
            }
        }
    }

    /// Read one whole batch: an opening line, its diagnostics, a closing line.
    ///
    /// Every line is required to be one whole JSON object, which is the property
    /// a consumer reading the stream a line at a time depends on.
    fn next_batch(&mut self) -> Batch {
        let opening = self.next_line("a batch to open");
        let DevStreamLine::Event(DevEvent::Tick { ts, path, .. }) = parse(&opening) else {
            panic!("a batch should open with a tick, got {}", opening);
        };

        let mut diagnostics = 0;
        loop {
            let line = self.next_line("a batch to close");
            match parse(&line) {
                DevStreamLine::Event(DevEvent::Idle { ok, .. }) => {
                    return Batch {
                        ts,
                        path,
                        diagnostics,
                        ok,
                    }
                }
                DevStreamLine::Diagnostic(_) => diagnostics += 1,
                DevStreamLine::Event(DevEvent::Tick { .. }) => {
                    panic!("a batch should close before the next one opens: {}", line)
                }
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.process.kill();
        let _ = self.process.wait();
    }
}

/// One batch, as a test cares about it.
struct Batch {
    ts: u64,
    path: String,
    diagnostics: usize,
    ok: bool,
}

/// Read `stdout` on its own thread so a test can wait with a timeout.
///
/// Reading inline would block forever when a batch never arrives, which is the
/// failure these tests exist to catch.
fn reader_thread(stdout: ChildStdout) -> Receiver<String> {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { return };
            if sender.send(line).is_err() {
                return;
            }
        }
    });
    receiver
}

/// Read one line of the stream, failing with the line itself if it is malformed.
fn parse(line: &str) -> DevStreamLine {
    DevStreamLine::parse(line)
        .unwrap_or_else(|error| panic!("a line should be one whole object: {}: {}", error, line))
}

/// A directory holding `main.mi` with the given source.
fn project(source: &str) -> (TempDir, PathBuf) {
    let directory = TempDir::new().expect("a temporary directory should be available");
    let path = directory.path().join("main.mi");
    std::fs::write(&path, source).expect("the source should be written");
    (directory, path)
}

/// A program whose only diagnostic is a warning.
const CALLS_A_DEPRECATED_FUNCTION: &str = concat!(
    "@deprecated(\"use bar\")\n",
    "fn foo()\n",
    "    return\n",
    "\n",
    "fn main()\n",
    "    foo()\n"
);

/// Overwrite `path`, making sure the modification time actually moves.
///
/// A filesystem whose timestamps have coarse resolution can record a rewrite
/// that happened within the same tick as no change at all, which would make the
/// test flaky for a reason that has nothing to do with the watch session.
fn rewrite(path: &Path, source: &str) {
    std::thread::sleep(Duration::from_millis(1100));
    let mut file = std::fs::File::create(path).expect("the source should be replaceable");
    file.write_all(source.as_bytes())
        .expect("the source should be written");
    file.sync_all().expect("the source should reach the disk");
}

#[test]
fn a_session_reports_the_current_state_before_anything_changes() {
    let (_directory, path) = project("fn main()\n    return\n");
    let mut session = Session::start(&path, "json");

    let batch = session.next_batch();
    assert_eq!(batch.ts, 0, "a session's first batch opens at zero");
    assert!(
        batch.path.ends_with("main.mi"),
        "the batch should name the file it checked, got {}",
        batch.path
    );
    assert!(batch.ok, "a program that compiles should report ok");
    assert_eq!(batch.diagnostics, 0, "a clean program has no diagnostics");
}

#[test]
fn a_failing_program_is_reported_as_a_batch_that_found_something() {
    let (_directory, path) = project("fn main()\n    let x = missing_function()\n");
    let mut session = Session::start(&path, "json");

    let batch = session.next_batch();
    assert!(!batch.ok, "a program that does not compile reports not ok");
    assert!(
        batch.diagnostics > 0,
        "a failing check should carry at least one diagnostic"
    );
}

#[test]
fn changing_the_watched_file_opens_another_batch() {
    let (_directory, path) = project("fn main()\n    return\n");
    let mut session = Session::start(&path, "json");

    let first = session.next_batch();
    assert_eq!(first.ts, 0);
    assert!(first.ok);

    rewrite(&path, "fn main()\n    let x = missing_function()\n");

    let second = session.next_batch();
    assert!(
        second.ts > first.ts,
        "a later batch opens later in the session: {} then {}",
        first.ts,
        second.ts
    );
    assert!(!second.ok, "the session should report the new state");
    assert!(second.diagnostics > 0);
}

#[test]
fn a_warning_closes_its_batch_as_ok() {
    let (_directory, path) = project(CALLS_A_DEPRECATED_FUNCTION);
    let mut session = Session::start(&path, "json");

    let batch = session.next_batch();
    assert!(
        batch.diagnostics > 0,
        "the deprecated call should be reported"
    );
    assert!(
        batch.ok,
        "a warning does not make a check fail, so the batch closes ok"
    );
}

#[test]
fn changing_a_neighbour_re_checks_the_watched_file() {
    // A module resolves its imports against the directory it sits in, so a
    // change to a file beside the watched one can change what the watched one
    // means. The session is meant to notice.
    let (directory, path) = project("fn main()\n    return\n");
    let neighbour = directory.path().join("helper.mi");
    std::fs::write(&neighbour, "fn helper()\n    return\n")
        .expect("the neighbour should be written");

    let mut session = Session::start(&path, "json");
    let first = session.next_batch();
    assert_eq!(first.ts, 0);

    rewrite(&neighbour, "fn helper()\n    let y = 1\n");

    let second = session.next_batch();
    assert!(
        second.ts > first.ts,
        "a neighbour changing should open another batch"
    );
    assert!(
        second.path.ends_with("main.mi"),
        "the batch should still be about the watched file, got {}",
        second.path
    );
}

#[test]
fn a_file_that_vanishes_and_returns_does_not_end_the_session() {
    // An editor that saves by replacing a file leaves a moment where the path
    // does not resolve. A session that died there would be useless.
    let (_directory, path) = project("fn main()\n    return\n");
    let mut session = Session::start(&path, "json");

    let first = session.next_batch();
    assert!(first.ok);

    std::thread::sleep(Duration::from_millis(1100));
    std::fs::remove_file(&path).expect("the source should be removable");
    rewrite(&path, "fn main()\n    let x = missing_function()\n");

    let second = session.next_batch();
    assert!(
        second.ts > first.ts,
        "the session should still be reporting after the file came back"
    );
    assert!(
        !second.ok,
        "the returned file should be checked as it now is"
    );
}

#[test]
fn a_quiet_session_reports_nothing() {
    let (_directory, path) = project("fn main()\n    return\n");
    let mut session = Session::start(&path, "json");

    session.next_batch();

    // Long enough for many polls to have found nothing. A session that reported
    // on a timer rather than on a change would have opened another batch here.
    match session.lines.recv_timeout(Duration::from_millis(2000)) {
        Err(RecvTimeoutError::Timeout) => {}
        Ok(line) => panic!(
            "a session with nothing to report should stay quiet: {}",
            line
        ),
        Err(RecvTimeoutError::Disconnected) => panic!("the session ended on its own"),
    }
}

#[test]
fn watching_a_file_that_is_not_there_fails_instead_of_waiting_for_it() {
    let directory = TempDir::new().expect("a temporary directory should be available");
    let missing = directory.path().join("absent.mi");

    let output = Command::new(assert_cmd::cargo_bin!("miri"))
        .arg("dev")
        .arg(&missing)
        .arg("--format")
        .arg("json")
        .env(
            "MIRI_STDLIB_PATH",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib"),
        )
        .output()
        .expect("the compiler binary should start");

    assert!(
        !output.status.success(),
        "watching a file that is not there should fail"
    );
    let reported = String::from_utf8(output.stderr).expect("stderr should be UTF-8");
    assert!(
        reported.contains("absent.mi"),
        "the failure should name the file, got {}",
        reported
    );
    assert!(
        output.stdout.is_empty(),
        "a session that never started should put nothing on the stream"
    );
}

#[test]
fn a_rendered_session_puts_no_stream_lines_on_stdout() {
    let (_directory, path) = project("fn main()\n    return\n");
    let mut session = Session::start(&path, "pretty");

    let line = session.next_line("the rendered summary");
    assert!(
        DevStreamLine::parse(&line).is_err(),
        "a rendered session should not write stream lines: {}",
        line
    );
    assert!(
        line.contains("Check passed"),
        "a rendered session should say what it found, got {}",
        line
    );
}
