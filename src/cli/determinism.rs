// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri determinism check` command: verify that artifacts are byte-reproducible.
//!
//! The command builds an input file twice in separate output directories and compares
//! the produced artifacts byte-for-byte. Drift indicates non-determinism.
//!
//! This module separates the work (comparison) from the writing (reporting), so a
//! long-lived server could invoke the work function later.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::cli::{serialize_envelope, ColorMode, Format};
use crate::diagnostics::codes::DiagnosticCode;
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic};
use crate::error::diagnostic::to_json;
use crate::error::CompilerError;
use crate::pipeline::{BuildOptions, Pipeline};

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The two builds produced byte-identical artifacts.
    DeterministicArtifacts,
    /// The two builds produced differing artifacts.
    NonDeterministicArtifacts,
    /// The input does not compile (reported via compiler diagnostics).
    BuildFailed,
}

/// A snapshot of an output directory tree, indexed by relative path.
///
/// Only file contents are compared; filesystem metadata (mtime, mode, symlinks) are ignored.
pub type Snapshot = BTreeMap<PathBuf, Vec<u8>>;

/// The kind of difference detected between two snapshots.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriftKind {
    /// Same path, same length, but bytes differ at this offset with these hex windows.
    BytesMismatch {
        offset: usize,
        hex_run1: String,
        hex_run2: String,
    },
    /// Same path, different lengths.
    LengthMismatch { len_run1: usize, len_run2: usize },
    /// Path present only in run 1.
    PresentInRun1Only,
    /// Path present only in run 2.
    PresentInRun2Only,
}

/// A single drift entry: a file that differs between two builds.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Drift {
    /// Relative path to the differing artifact.
    pub path: PathBuf,
    /// The kind of difference.
    pub kind: DriftKind,
}

/// Take a recursive snapshot of a directory tree, indexed by paths relative to that directory.
fn snapshot_directory(root: &Path) -> Result<Snapshot, std::io::Error> {
    let mut snapshot = Snapshot::new();
    let mut stack = vec![PathBuf::from("")];

    while let Some(rel_dir) = stack.pop() {
        let abs_dir = root.join(&rel_dir);
        if !abs_dir.is_dir() {
            continue;
        }
        for entry in fs::read_dir(&abs_dir)? {
            let entry = entry?;
            let file_type = entry.file_type()?;
            let file_name = entry.file_name();
            let rel_path = if rel_dir.as_os_str().is_empty() {
                PathBuf::from(&file_name)
            } else {
                rel_dir.join(&file_name)
            };

            // Skip symlinks; only process regular files and directories.
            if file_type.is_symlink() {
                continue;
            }

            if file_type.is_dir() {
                stack.push(rel_path);
            } else if file_type.is_file() {
                let contents = fs::read(entry.path())?;
                snapshot.insert(rel_path, contents);
            }
        }
    }

    Ok(snapshot)
}

/// Compare a single path across two snapshots and collect any drift.
fn compare_path(
    drifts: &mut Vec<Drift>,
    path: &Path,
    bytes1: Option<&Vec<u8>>,
    bytes2: Option<&Vec<u8>>,
) {
    match (bytes1, bytes2) {
        (Some(b1), Some(b2)) => {
            if b1 != b2 {
                if b1.len() != b2.len() {
                    drifts.push(Drift {
                        path: path.to_path_buf(),
                        kind: DriftKind::LengthMismatch {
                            len_run1: b1.len(),
                            len_run2: b2.len(),
                        },
                    });
                } else if let Some(first_diff) = b1.iter().zip(b2.iter()).position(|(a, b)| a != b)
                {
                    let window_size = 16;
                    let start = first_diff.saturating_sub(window_size / 2);
                    let end1 = (start + window_size).min(b1.len());
                    let end2 = (start + window_size).min(b2.len());

                    let hex1 = hex_window(&b1[start..end1]);
                    let hex2 = hex_window(&b2[start..end2]);

                    drifts.push(Drift {
                        path: path.to_path_buf(),
                        kind: DriftKind::BytesMismatch {
                            offset: first_diff,
                            hex_run1: hex1,
                            hex_run2: hex2,
                        },
                    });
                }
            }
        }
        (None, Some(_)) => {
            drifts.push(Drift {
                path: path.to_path_buf(),
                kind: DriftKind::PresentInRun2Only,
            });
        }
        (Some(_), None) => {
            drifts.push(Drift {
                path: path.to_path_buf(),
                kind: DriftKind::PresentInRun1Only,
            });
        }
        (None, None) => {
            // This case only occurs when a path is absent from both snapshots,
            // which is impossible if that path is in the union of both key sets.
            // This is a safety valve: if the caller violates that contract,
            // we silently do nothing (no drift to report). The function is called
            // only from compare_snapshots which maintains the invariant.
        }
    }
}

pub fn compare_snapshots(run1: &Snapshot, run2: &Snapshot) -> Vec<Drift> {
    let mut drifts = Vec::new();

    // Collect all unique paths from both snapshots.
    let all_paths: std::collections::BTreeSet<_> = run1.keys().chain(run2.keys()).collect();

    for path in all_paths {
        compare_path(&mut drifts, path, run1.get(path), run2.get(path));
    }

    drifts
}

/// Render a small hex window around a byte offset for diagnostic output.
fn hex_window(bytes: &[u8]) -> String {
    let hex_pairs: Vec<String> = bytes.iter().map(|b| format!("{:02x}", b)).collect();
    hex_pairs.join(" ")
}

/// Format a drift into a message and help text.
fn format_drift(drift: &Drift) -> (String, String) {
    match &drift.kind {
        DriftKind::BytesMismatch {
            offset,
            hex_run1,
            hex_run2,
        } => (
            format!("bytes differ at offset {}", offset),
            format!("run 1: {}\nrun 2: {}", hex_run1, hex_run2),
        ),
        DriftKind::LengthMismatch { len_run1, len_run2 } => (
            format!(
                "length mismatch (run 1: {} bytes, run 2: {} bytes)",
                len_run1, len_run2
            ),
            "Non-deterministic artifacts suggest unordered iteration reached emitted bytes. \
             Verify that all collection iterations in the compiler use ordered types."
                .to_string(),
        ),
        DriftKind::PresentInRun1Only => (
            "present in run 1 but missing in run 2".to_string(),
            "Non-deterministic artifacts suggest unordered iteration reached emitted bytes. \
             Verify that all collection iterations in the compiler use ordered types."
                .to_string(),
        ),
        DriftKind::PresentInRun2Only => (
            "present in run 2 but missing in run 1".to_string(),
            "Non-deterministic artifacts suggest unordered iteration reached emitted bytes. \
             Verify that all collection iterations in the compiler use ordered types."
                .to_string(),
        ),
    }
}

/// Convert a Drift to a JsonDiagnostic with code MER_BLD_003.
pub fn drift_diagnostic(drift: &Drift) -> JsonDiagnostic {
    let (message, help) = format_drift(drift);
    JsonDiagnostic {
        severity: "error".to_string(),
        code: Some(
            DiagnosticCode::BldNonDeterministicArtifact
                .as_str()
                .to_string(),
        ),
        message,
        path: Some(drift.path.display().to_string()),
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: Some(help),
        fix_safety: None,
        repair: None,
        related: vec![],
        preexisting: None,
    }
}

/// Build an input file and return a snapshot of its output directory.
fn build_and_snapshot(
    path: &Path,
    source: &str,
    verify_mir: bool,
    opts: &BuildOptions,
) -> Result<(Snapshot, PathBuf), CompilerError> {
    let mut pipeline = Pipeline::new().with_verify_mir(verify_mir);
    let absolute = path.canonicalize().map_err(CompilerError::Io)?;
    if let Some(directory) = absolute.parent() {
        pipeline = pipeline.with_source_dir(directory.to_path_buf());
    }
    pipeline = pipeline.with_source_path(absolute.display().to_string());

    let result = pipeline.build(source, opts)?;

    // The output is either a single executable or a directory (web-gpu bundle).
    // Snapshot the output directory.
    let output_dir = if result.is_file() {
        result
            .parent()
            .unwrap_or_else(|| Path::new(""))
            .to_path_buf()
    } else {
        result.clone()
    };

    let snapshot = snapshot_directory(&output_dir).map_err(CompilerError::Io)?;

    Ok((snapshot, result))
}

/// Run the determinism check on `path` and return what it found.
///
/// Nothing here writes to a stream or ends the process, so the same call serves
/// the command line and a request over a long-lived connection.
/// Build options that write this run's artifacts into `directory`.
///
/// Both runs share every setting but the output location, so any difference in
/// what they produce comes from the compiler rather than from how it was asked
/// to build.
fn options_for_run(directory: &Path, basename: &str, opts: &BuildOptions) -> BuildOptions {
    BuildOptions {
        out_path: Some(directory.join(basename)),
        release: opts.release,
        opt_level: opts.opt_level,
        cpu_backend: opts.cpu_backend,
        target: opts.target,
        emit_native_host: opts.emit_native_host,
    }
}

/// Envelope for a comparison that found no difference.
fn reproducible_envelope(elapsed_ms: u64) -> DiagnosticsEnvelope {
    DiagnosticsEnvelope::new(JsonCommand::Determinism, true, vec![])
        .with_exit_code(0)
        .with_duration_ms(elapsed_ms)
}

/// Envelope carrying one diagnostic per drift the comparison found.
fn drift_envelope(drifts: Vec<Drift>, elapsed_ms: u64) -> DiagnosticsEnvelope {
    let diagnostics = drifts.iter().map(drift_diagnostic).collect();
    DiagnosticsEnvelope::new(JsonCommand::Determinism, false, diagnostics)
        .with_exit_code(1)
        .with_duration_ms(elapsed_ms)
}

/// Envelope for an input that does not compile.
///
/// The compiler's own diagnostics are reported as it reported them, keeping
/// their codes and source locations: a program that fails to build has not been
/// shown to be non-deterministic, so this never carries the drift code.
fn build_failure_envelope(
    error: &CompilerError,
    source: &str,
    path: &Path,
    elapsed_ms: u64,
) -> DiagnosticsEnvelope {
    let source_path = path.display().to_string();
    let diagnostics = error
        .to_diagnostics()
        .into_iter()
        .map(|diagnostic| to_json(&diagnostic, source, Some(source_path.as_str())))
        .collect();
    DiagnosticsEnvelope::new(JsonCommand::Determinism, false, diagnostics)
        .with_exit_code(1)
        .with_duration_ms(elapsed_ms)
}

/// Envelope for a failure of this command's own machinery rather than of the
/// build it was asked to check.
fn command_failure_envelope(message: &str, elapsed_ms: u64) -> DiagnosticsEnvelope {
    let diagnostic = JsonDiagnostic {
        severity: "error".to_string(),
        code: None,
        message: message.to_string(),
        path: None,
        line: None,
        column: None,
        length: None,
        expected: None,
        actual: None,
        help: None,
        fix_safety: None,
        repair: None,
        related: vec![],
        preexisting: None,
    };
    DiagnosticsEnvelope::new(JsonCommand::Determinism, false, vec![diagnostic])
        .with_exit_code(1)
        .with_duration_ms(elapsed_ms)
}

/// Build `path` twice into clean directories and report whether the artifacts
/// agree byte for byte.
///
/// Nothing here writes to a stream or ends the process, so the same call serves
/// the command line and a request over a long-lived connection.
pub fn check(
    path: &Path,
    source: &str,
    verify_mir: bool,
    build_opts: &BuildOptions,
) -> (Outcome, DiagnosticsEnvelope) {
    let start = std::time::Instant::now();
    // The same output basename in both directories keeps the relative paths of
    // the two artifact trees comparable.
    let basename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("output")
        .to_string();

    let (Ok(first_dir), Ok(second_dir)) = (tempfile::tempdir(), tempfile::tempdir()) else {
        let elapsed_ms = start.elapsed().as_millis() as u64;
        return (
            Outcome::BuildFailed,
            command_failure_envelope("failed to create temporary directories", elapsed_ms),
        );
    };

    let first = build_and_snapshot(
        path,
        source,
        verify_mir,
        &options_for_run(first_dir.path(), &basename, build_opts),
    );
    let second = build_and_snapshot(
        path,
        source,
        verify_mir,
        &options_for_run(second_dir.path(), &basename, build_opts),
    );

    match (first, second) {
        (Ok((first_snapshot, _)), Ok((second_snapshot, _))) => {
            let drifts = compare_snapshots(&first_snapshot, &second_snapshot);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if drifts.is_empty() {
                (
                    Outcome::DeterministicArtifacts,
                    reproducible_envelope(elapsed_ms),
                )
            } else {
                (
                    Outcome::NonDeterministicArtifacts,
                    drift_envelope(drifts, elapsed_ms),
                )
            }
        }
        (Err(error), _) | (_, Err(error)) => {
            let elapsed_ms = start.elapsed().as_millis() as u64;
            (
                Outcome::BuildFailed,
                build_failure_envelope(&error, source, path, elapsed_ms),
            )
        }
    }
}

/// Check the file at `path` and write the result.
///
/// Diagnostics go to stderr and the closing summary to stdout.
pub fn run(
    path: &Path,
    format: Format,
    verify_mir: bool,
    _color_mode: ColorMode,
    build_opts: &BuildOptions,
) -> Outcome {
    let Some(source) =
        crate::cli::source::read_or_report(path, JsonCommand::Determinism, format, _color_mode)
    else {
        return Outcome::BuildFailed;
    };

    let (outcome, envelope) = check(path, &source, verify_mir, build_opts);

    if format == Format::Json {
        println!("{}", serialize_envelope(&envelope));
    } else {
        for diagnostic in &envelope.diagnostics {
            eprintln!("{}: {}", diagnostic.severity, diagnostic.message);
            if let Some(help) = &diagnostic.help {
                eprintln!("  {}", help);
            }
        }
        if envelope.ok {
            println!("Determinism check passed. Artifacts are byte-identical.");
        }
    }

    outcome
}
