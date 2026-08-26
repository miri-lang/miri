// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The line shapes of the watch stream, and the writers that put them on a
//! stream.
//!
//! A watch session reports what it found one JSON object per line rather than
//! one envelope per batch, so a consumer can act on a diagnostic the moment it
//! arrives instead of waiting for the batch to close. A batch opens with a
//! `tick`, carries one line per diagnostic, and closes with an `idle`.
//!
//! A diagnostic line is a bare [`JsonDiagnostic`] — the same shape it has inside
//! an envelope, so a consumer reads one thing rather than two. The framing lines
//! are told apart from it by carrying an `event` member, which a diagnostic
//! never has.
//!
//! This module takes plain data and never names `crate::error` or `crate::cli`:
//! it is the inner layer, and the watch loop that drives it lives outside.

use serde::{Deserialize, Serialize};
use std::io::Write;

use crate::diagnostics::json::{JsonDiagnostic, SCHEMA_VERSION};

/// A framing line: the opening or closing of one batch.
///
/// The `event` member is both the discriminant and the thing that separates a
/// framing line from a diagnostic. It is an enum rather than a string so that
/// an unrecognised event name is a parse failure instead of a silently accepted
/// line — the format is published, and a consumer that misreads it should be
/// told so.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "camelCase", deny_unknown_fields)]
pub enum DevEvent {
    /// Opens a batch. Everything up to the next `idle` belongs to it.
    #[serde(rename = "tick", rename_all = "camelCase")]
    Tick {
        /// The version of the line shapes in this batch.
        ///
        /// It rides on the opening line of every batch rather than being stated
        /// once, because a consumer that attaches to a running session — the
        /// `tail -F` case — never saw the beginning of the stream and would
        /// otherwise have to guess.
        schema_version: u32,
        /// Milliseconds elapsed since the watch session began.
        ///
        /// Monotonic, not wall-clock: it says how far into the session the batch
        /// opened, so the first batch is always `0`. A consumer that needs a
        /// wall-clock instant adds this to when it started the session.
        ts: u64,
        /// The file the batch re-checked.
        path: String,
    },
    /// Closes a batch.
    #[serde(rename = "idle", rename_all = "camelCase")]
    Idle {
        /// Whether the check succeeded. Warnings do not make this false.
        ok: bool,
        /// How long the check took, in milliseconds.
        duration_ms: u64,
    },
}

impl DevEvent {
    /// Open a batch for `path` at `ts` milliseconds into the session.
    pub fn tick(ts: u64, path: impl Into<String>) -> Self {
        DevEvent::Tick {
            schema_version: SCHEMA_VERSION,
            ts,
            path: path.into(),
        }
    }

    /// Close a batch that took `duration_ms` and ended `ok`.
    pub fn idle(ok: bool, duration_ms: u64) -> Self {
        DevEvent::Idle { ok, duration_ms }
    }
}

/// One line of the watch stream, as a consumer reads it back.
#[derive(Debug, Clone, PartialEq)]
pub enum DevStreamLine {
    /// A framing line opening or closing a batch.
    Event(DevEvent),
    /// A diagnostic belonging to the batch currently open.
    Diagnostic(Box<JsonDiagnostic>),
}

impl DevStreamLine {
    /// Read one line of the stream.
    ///
    /// The `event` member decides which shape the line is. Dispatching on it
    /// rather than trying each shape in turn is what lets a malformed framing
    /// line report why it is malformed, instead of reporting that it failed to
    /// be a diagnostic.
    pub fn parse(line: &str) -> Result<Self, serde_json::Error> {
        let value: serde_json::Value = serde_json::from_str(line)?;
        if value.get("event").is_some() {
            serde_json::from_value(value).map(DevStreamLine::Event)
        } else {
            serde_json::from_value(value).map(|d| DevStreamLine::Diagnostic(Box::new(d)))
        }
    }
}

/// Write one framing line.
pub fn write_event(output: &mut impl Write, event: &DevEvent) -> std::io::Result<()> {
    write_line(output, &serde_json::to_string(event)?)
}

/// Write one diagnostic line.
pub fn write_diagnostic(
    output: &mut impl Write,
    diagnostic: &JsonDiagnostic,
) -> std::io::Result<()> {
    write_line(output, &serde_json::to_string(diagnostic)?)
}

/// Put one already-serialized object on the stream, newline-terminated.
///
/// The whole line, terminator included, goes out in a single `write!` from a
/// single-threaded writer, which is what keeps a consumer from ever reading a
/// half-written object. Flushing here rather than at the end of a batch is what
/// makes the stream worth reading line by line.
fn write_line(output: &mut impl Write, body: &str) -> std::io::Result<()> {
    writeln!(output, "{}", body)?;
    output.flush()
}
