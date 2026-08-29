// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! JSON schema types for structured diagnostic output.
//!
//! This module defines the serializable DTO types for emitting diagnostics as JSON.
//! It lives in the inner diagnostics layer and references only Severity and DiagnosticCode,
//! never types from `src/error/`.

use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

/// Top-level JSON envelope wrapping a compilation or test command's diagnostics.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiagnosticsEnvelope {
    /// Schema version (always 1 in this version).
    pub schema_version: u32,
    /// True when compilation/check succeeded; false otherwise.
    pub ok: bool,
    /// The command that generated this envelope.
    pub command: JsonCommand,
    /// Diagnostics (errors, warnings, notes).
    pub diagnostics: Vec<JsonDiagnostic>,
    /// Path to the build artifact (build/run commands only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<String>,
    /// Process exit code (build/run/test commands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    /// Last N bytes of stdout (build/run/test commands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_tail: Option<String>,
    /// Last N bytes of stderr (build/run/test commands).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_tail: Option<String>,
    /// True if stdout was truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stdout_truncated: Option<bool>,
    /// True if stderr was truncated.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr_truncated: Option<bool>,
    /// Elapsed time in milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Test summary (test command only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tests: Option<JsonTestSummary>,
    /// Explanation of a diagnostic code (explain command only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub explanation: Option<JsonExplanation>,
    /// Canonical source read back by the view command (view command only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub view: Option<JsonView>,
    /// Patch results: applied edits and revalidation count (patch command only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub patch: Option<JsonPatch>,
    /// Skills from the embedded catalogue (skill command only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub skills: Option<Vec<JsonSkill>>,
}

impl DiagnosticsEnvelope {
    /// Create a new envelope with the current schema version and minimal required fields.
    /// All optional fields default to None.
    pub fn new(command: JsonCommand, ok: bool, diagnostics: Vec<JsonDiagnostic>) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            ok,
            command,
            diagnostics,
            artifact: None,
            exit_code: None,
            stdout_tail: None,
            stderr_tail: None,
            stdout_truncated: None,
            stderr_truncated: None,
            duration_ms: None,
            tests: None,
            explanation: None,
            view: None,
            patch: None,
            skills: None,
        }
    }

    /// Set the exit code.
    pub fn with_exit_code(mut self, code: i32) -> Self {
        self.exit_code = Some(code);
        self
    }

    /// Set the artifact path.
    pub fn with_artifact(mut self, path: String) -> Self {
        self.artifact = Some(path);
        self
    }

    /// Set the elapsed time in milliseconds.
    pub fn with_duration_ms(mut self, ms: u64) -> Self {
        self.duration_ms = Some(ms);
        self
    }

    /// Set the stdout tail and truncation flag.
    pub fn with_stdout(mut self, tail: String, truncated: bool) -> Self {
        self.stdout_tail = Some(tail);
        self.stdout_truncated = Some(truncated);
        self
    }

    /// Set the stderr tail and truncation flag.
    pub fn with_stderr(mut self, tail: String, truncated: bool) -> Self {
        self.stderr_tail = Some(tail);
        self.stderr_truncated = Some(truncated);
        self
    }

    /// Set the test summary.
    pub fn with_tests(mut self, summary: JsonTestSummary) -> Self {
        self.tests = Some(summary);
        self
    }

    /// Set the diagnostic code explanation.
    pub fn with_explanation(mut self, explanation: JsonExplanation) -> Self {
        self.explanation = Some(explanation);
        self
    }

    /// Set the canonical source a view read back.
    pub fn with_view(mut self, view: JsonView) -> Self {
        self.view = Some(view);
        self
    }

    /// Set the patch results.
    pub fn with_patch(mut self, patch: JsonPatch) -> Self {
        self.patch = Some(patch);
        self
    }

    /// Set the skills list.
    pub fn with_skills(mut self, skills: Vec<JsonSkill>) -> Self {
        self.skills = Some(skills);
        self
    }
}

/// What a `miri view` call read back.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonView {
    /// Which shape was asked for: `fn`, `outline`, or `around`.
    pub shape: String,
    /// The canonical source. Every span below indexes these bytes.
    pub text: String,
    /// Where each declaration sits within `text`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spans: Vec<JsonViewSpan>,
}

/// One declaration's position within a view's text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonViewSpan {
    /// Byte offset where the declaration starts.
    pub start: usize,
    /// Byte offset one past the declaration's last byte.
    pub end: usize,
    /// What kind of declaration this is, such as `function` or `class`.
    pub kind: String,
    /// The declared name, when the declaration has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

/// Results of a patch command: applied edits and metadata.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonPatch {
    /// Individual edits applied in the patch (each with raw byte range and replacement).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub edits: Vec<JsonPatchEdit>,
    /// Number of revalidations performed.
    pub revalidations: u32,
    /// Whether the file was written to disk.
    pub file_written: bool,
}

/// A single edit applied by the patch command.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonPatchEdit {
    /// Byte offset where the replacement starts in the raw source.
    pub start: usize,
    /// Byte offset one past the last replaced byte.
    pub end: usize,
    /// The replacement text.
    pub replacement: String,
}

/// Command type for the JSON envelope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JsonCommand {
    Check,
    Build,
    Run,
    Test,
    Explain,
    Fix,
    View,
    Patch,
    Determinism,
    Skill,
}

/// A single diagnostic (error, warning, or note).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonDiagnostic {
    /// Severity level: "error", "warning", or "note".
    pub severity: String,
    /// Diagnostic code (e.g., "MER_TYP_010").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Human-readable message.
    pub message: String,
    /// Source file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// 1-indexed line number in source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// 1-indexed column number in source.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
    /// Length in bytes of the error span.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub length: Option<usize>,
    /// Expected type/value for type mismatch errors.
    /// Note: Only populated for explicitly-typed error variants (TypeMismatch, ArityMismatch, etc).
    /// Family-coded errors (those carrying a DiagnosticCode and message string) do not populate
    /// this field in schema version 1, even if the message describes a type mismatch.
    /// See notes/PLAN.md for the follow-up to extract structured pairs from message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    /// Actual type/value for type mismatch errors.
    /// Note: Only populated for explicitly-typed error variants (TypeMismatch, ArityMismatch, etc).
    /// Family-coded errors (those carrying a DiagnosticCode and message string) do not populate
    /// this field in schema version 1, even if the message describes a type mismatch.
    /// See notes/PLAN.md for the follow-up to extract structured pairs from message text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actual: Option<String>,
    /// Actionable help text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub help: Option<String>,
    /// The fix-safety level of this diagnostic's repair (if any), indicating the
    /// risk level of applying the repair. One of: format-only, behavior-preserving,
    /// local-edit, api-changing, target-changing, or requires-human-review.
    /// Absent or null if the diagnostic carries no repair.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fix_safety: Option<String>,
    /// The repair available for this diagnostic, if the compiler determined one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub repair: Option<JsonRepair>,
    /// Related diagnostics (notes from the original).
    #[serde(default)]
    pub related: Vec<JsonRelated>,
}

/// A repair suggestion (reserved for Task 4).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonRepair {
    pub id: String,
    pub summary: String,
    /// The edits that carry out this repair, in source order.
    ///
    /// Never empty: a repair exists only when the compiler determined the edit,
    /// so a repair that describes no change is not representable.
    pub edits: Vec<JsonEdit>,
}

/// A single edit to apply as part of a repair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonEdit {
    /// File path (absolute or relative).
    pub path: String,
    /// Start byte offset (inclusive).
    pub start: usize,
    /// End byte offset (exclusive).
    pub end: usize,
    /// Replacement text.
    pub replacement: String,
}

/// A related diagnostic (typically a note).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonRelated {
    /// Severity: "error", "warning", or "note".
    pub severity: String,
    /// Message text.
    pub message: String,
    /// Diagnostic code (if applicable).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub code: Option<String>,
    /// Source file path.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    /// Line number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    /// Column number.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub column: Option<usize>,
}

/// A diagnostic code's documentation, rendered for machine consumption.
///
/// `code`, `title`, `severity` and `reserved` are read from the code registry,
/// which is their single source of truth. The remaining fields are parsed from
/// the documentation file embedded for that code. A retired code carries
/// `reserved: true` and no example pair: the check no longer runs, so there is
/// nothing to reproduce or repair.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonExplanation {
    /// Wire string of the code being explained (e.g. "MER_TYP_010").
    pub code: String,
    /// Short title from the registry.
    pub title: String,
    /// Severity: "error", "warning", or "note".
    pub severity: String,
    /// True when the code is retired and no longer emitted.
    pub reserved: bool,
    /// The rule the check enforces.
    pub rule: String,
    /// Source showing the problem. Absent for a reserved code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_before: Option<String>,
    /// The same source, repaired. Absent for a reserved code.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub example_after: Option<String>,
    /// Relative path to the reference page covering this area.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference: Option<String>,
}

/// Test summary (mirrors TestSummary in camelCase).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonTestSummary {
    pub total: usize,
    pub passed: usize,
    pub failed: usize,
    pub ignored: usize,
    pub results: Vec<JsonTestResult>,
    pub rejected_files: Vec<JsonRejectedFile>,
}

/// Individual test result in summary.
/// Mirrors TestResult from test_runner/mod.rs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonTestResult {
    pub path: String,
    pub name: String,
    /// Test outcome: passed, failed, ignored, expected_failure, unexpected_pass, crashed, runner_fault.
    /// These values come from the Outcome enum's snake_case serialization.
    pub outcome: String,
    /// The ignore/xfail reason, the captured stderr, or the fault description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// A file rejected during test discovery, and why.
/// Mirrors RejectedFile from test_runner/discovery.rs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonRejectedFile {
    pub path: String,
    /// Rejection reason: unparseable, declares_main, or top_level_statements.
    /// These values come from the RejectionReason enum's snake_case serialization.
    pub reason: String,
}

/// A skill from the embedded catalogue.
///
/// Which members are present says which caller asked: `installedPath` and
/// `unchanged` come from writing a skill out, `body` from a request that
/// carries the text itself, and the rest are always there.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JsonSkill {
    /// Skill name
    pub name: String,
    /// One-line description
    pub description: String,
    /// Compiler version
    pub compiler_version: String,
    /// Path where the skill was installed (install command only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_path: Option<String>,
    /// True if the file was identical and not rewritten (install command only)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unchanged: Option<bool>,
    /// The skill's markdown body, without its header (`skillsGet` only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub body: Option<String>,
}
