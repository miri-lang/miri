// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{
    Diagnostic, DiagnosticBuilder, Reportable, Severity, BUG_REPORT_URL,
};
use crate::error::format::{format_diagnostic, format_diagnostic_with_color, ColorChoice};
use crate::error::lowering::LoweringError;
use crate::error::syntax::{SyntaxError, SyntaxErrors};
use crate::error::type_error::TypeError;
use thiserror::Error;

/// A diagnostic carrying its registered code, so a machine consumer can match
/// on it rather than on the sentence.
fn coded_diag(code: DiagnosticCode, message: String, help: Option<String>) -> Diagnostic {
    let mut builder = DiagnosticBuilder::error(code.title())
        .code(code.as_str())
        .message(message);
    if let Some(help) = help {
        builder = builder.help(help);
    }
    builder.build()
}

fn simple_diag(title: &str, message: String, help: Option<String>) -> Diagnostic {
    Diagnostic {
        severity: Severity::Error,
        code: None,
        title: title.to_string(),
        message,
        span: None,
        help,
        notes: Vec::new(),
        source_override: None,
        expected: None,
        actual: None,
        repair: None,
    }
}

/// Top-level error type encompassing all compiler pipeline errors.
#[derive(Error, Debug)]
pub enum CompilerError {
    #[error("I/O Error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Lexer Error: {0}")]
    Lexer(SyntaxError),

    /// Every syntax fault one parse reported.
    ///
    /// The parser resynchronises at top-level declaration boundaries, so a file
    /// broken in three declarations is rejected once and names all three.
    #[error("Parser Error: {0}")]
    Parser(SyntaxErrors),

    #[error("Type Error: {0}")]
    Type(Box<TypeError>),

    #[error("Type Errors: {errors:?}")]
    TypeErrors {
        errors: Vec<TypeError>,
        warnings: Vec<Diagnostic>,
    },

    #[error("File not found: {0}")]
    FileNotFound(String),

    #[error("Internal compiler error: {0}")]
    Internal(String),

    #[error("Codegen Error: {0}")]
    Codegen(String),

    #[error("Lowering Error: {0}")]
    Lowering(LoweringError),

    #[error("Runtime Error: {0}")]
    Runtime(String),

    #[error("MIR Verification Error: {0}")]
    MirVerification(String),

    /// A command refused the work it was asked to do, reported under a
    /// registered code.
    ///
    /// The stringly-typed variants above predate the diagnostic registry and
    /// carry a title where a code belongs. A refusal raised from now on travels
    /// with the code the caller matches on.
    #[error("{message}")]
    Coded {
        code: DiagnosticCode,
        message: String,
        help: Option<String>,
    },
}

impl CompilerError {
    /// Converts this error into a Vec of Diagnostics.
    ///
    /// This is the conversion funnel point: every CompilerError variant maps to
    /// one or more Diagnostic values. An EXHAUSTIVE match is required (no `_ =>`).
    pub fn to_diagnostics(&self) -> Vec<Diagnostic> {
        match self {
            CompilerError::Lexer(e) => vec![e.to_diagnostic()],
            CompilerError::Parser(errors) => errors.iter().map(|e| e.to_diagnostic()).collect(),
            CompilerError::Type(e) => vec![e.to_diagnostic()],
            CompilerError::TypeErrors { errors, warnings } => {
                let mut diags: Vec<Diagnostic> = warnings.clone();
                diags.extend(errors.iter().map(|e| e.to_diagnostic()));
                diags
            }
            CompilerError::Lowering(e) => vec![e.to_diagnostic()],
            CompilerError::Io(e) => vec![coded_diag(
                DiagnosticCode::BldInputNotReadable,
                format!("{}", e),
                None,
            )],
            CompilerError::FileNotFound(path) => vec![coded_diag(
                DiagnosticCode::BldInputNotReadable,
                format!("File not found: {}", path),
                None,
            )],
            CompilerError::Internal(msg) => vec![simple_diag(
                "Internal Compiler Error",
                msg.clone(),
                Some(format!("Please report this at {}", BUG_REPORT_URL)),
            )],
            // A family code, matching how the type checker's call sites are
            // grouped: the code says which stage refused, the message carries
            // the individual case. The backend's own richer `CodegenError`
            // codes do not survive the stringly-typed variant this arm reports.
            CompilerError::Codegen(msg) => vec![coded_diag(
                DiagnosticCode::CgInternalCodegenError,
                msg.clone(),
                None,
            )],
            CompilerError::Runtime(msg) => {
                vec![simple_diag("Runtime Error", msg.clone(), None)]
            }
            CompilerError::MirVerification(msg) => vec![coded_diag(
                DiagnosticCode::MirValidationFailed,
                msg.clone(),
                Some(
                    "This indicates a bug in MIR lowering or Perceus RC insertion. \
                     Please report it."
                        .to_string(),
                ),
            )],
            CompilerError::Coded {
                code,
                message,
                help,
            } => vec![coded_diag(*code, message.clone(), help.clone())],
        }
    }

    /// Formats this error for terminal display using the given source code.
    ///
    /// All variants are routed through [`format_diagnostic_full`] to ensure
    /// consistent formatting and TTY-aware color output.
    pub fn report(&self, source: &str) -> String {
        self.report_with_path(source, None)
    }

    /// Like [`report`](Self::report), but includes the entry-point file path
    /// in error locations when no per-diagnostic `source_override` is set.
    pub fn report_with_path(&self, source: &str, source_path: Option<&str>) -> String {
        let fmt = |diag: &Diagnostic| format_diagnostic(source, diag, source_path);
        self.to_diagnostics()
            .iter()
            .map(&fmt)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Like [`report_with_path`](Self::report_with_path), but respects the given color choice.
    pub fn report_with_path_and_color(
        &self,
        source: &str,
        source_path: Option<&str>,
        color_choice: ColorChoice,
    ) -> String {
        let fmt = |diag: &Diagnostic| {
            format_diagnostic_with_color(source, diag, source_path, color_choice)
        };
        self.to_diagnostics()
            .iter()
            .map(&fmt)
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The variants that still report without a registered code.
    ///
    /// Both are constructed nowhere in the compiler, so no code was minted for
    /// them: a code that can never be emitted is dead weight in a write-once
    /// registry. Retiring the variants, or coding them if a caller appears, is
    /// recorded as a follow-up. The list exists so it can only shrink — a new
    /// uncoded variant fails the assertion below rather than joining them.
    const UNCODED_UNREACHABLE_VARIANTS: [&str; 2] = ["Internal", "Runtime"];

    /// The variants whose diagnostic is built here rather than delegated.
    ///
    /// `Lexer`, `Parser`, `Type`, `TypeErrors` and `Lowering` hand off to error
    /// types that carry a `DiagnosticCode` by construction, so they cannot lose
    /// one. These build a `Diagnostic` in this file, which is where a code went
    /// missing. `to_diagnostics` matches exhaustively, so a new variant fails
    /// the build there and arrives here to be listed.
    fn locally_built_variants() -> Vec<(&'static str, CompilerError)> {
        vec![
            ("Io", CompilerError::Io(std::io::Error::other("disk gone"))),
            ("FileNotFound", CompilerError::FileNotFound("a.mi".into())),
            ("Internal", CompilerError::Internal("ice".into())),
            ("Codegen", CompilerError::Codegen("backend".into())),
            ("Runtime", CompilerError::Runtime("trap".into())),
            (
                "MirVerification",
                CompilerError::MirVerification("unbalanced".into()),
            ),
            (
                "Coded",
                CompilerError::Coded {
                    code: DiagnosticCode::BldNothingToRun,
                    message: "nothing to run".into(),
                    help: None,
                },
            ),
        ]
    }

    #[test]
    fn test_every_reachable_variant_reports_a_registered_code() {
        for (name, error) in locally_built_variants() {
            if UNCODED_UNREACHABLE_VARIANTS.contains(&name) {
                continue;
            }
            for diagnostic in error.to_diagnostics() {
                assert!(
                    diagnostic.code.is_some(),
                    "CompilerError::{} reports a diagnostic with no code",
                    name
                );
            }
        }
    }

    #[test]
    fn test_uncoded_variants_are_still_the_ones_listed() {
        for name in UNCODED_UNREACHABLE_VARIANTS {
            let (_, error) = locally_built_variants()
                .into_iter()
                .find(|(variant, _)| *variant == name)
                .expect("listed variant must exist");
            assert!(
                error.to_diagnostics().iter().all(|d| d.code.is_none()),
                "CompilerError::{} now carries a code; drop it from the list",
                name
            );
        }
    }

    #[test]
    fn test_coded_variant_carries_the_code_and_its_title() {
        let error = CompilerError::Coded {
            code: DiagnosticCode::BldNothingToRun,
            message: "nothing to run".into(),
            help: None,
        };
        let diagnostics = error.to_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, Some("MER_BLD_021"));
        assert_eq!(diagnostics[0].title, "Nothing to Run");
    }

    /// The backend's IR is compiler internals; it must not reach the published
    /// envelope unless someone debugging the backend asks for it.
    #[test]
    fn test_codegen_message_is_reported_verbatim() {
        let error = CompilerError::Codegen("Duplicate definition of identifier: f".into());
        let diagnostics = error.to_diagnostics();
        assert_eq!(diagnostics.len(), 1);
        assert!(!diagnostics[0].message.contains("Cranelift IR"));
    }
}
