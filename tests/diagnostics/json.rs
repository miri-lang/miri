// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

#[cfg(test)]
mod tests {
    use miri::diagnostics::Severity;
    use miri::error::compiler::CompilerError;
    use miri::error::diagnostic::DiagnosticBuilder;

    #[test]
    fn test_diagnostic_expected_actual_fields() {
        let diag = DiagnosticBuilder::error("Type Mismatch")
            .message("Expected Int but got Bool")
            .build();

        // Test that fields exist and default to None
        assert!(diag.expected.is_none());
        assert!(diag.actual.is_none());
    }

    #[test]
    fn test_diagnostic_with_expected_actual() {
        let diag = DiagnosticBuilder::error("Type Mismatch")
            .message("Expected Int but got Bool")
            .expected("Int")
            .actual("Bool")
            .build();

        assert_eq!(diag.expected, Some("Int".to_string()));
        assert_eq!(diag.actual, Some("Bool".to_string()));
    }

    #[test]
    fn test_compiler_error_to_diagnostics_io() {
        let err = CompilerError::Io(std::io::Error::new(std::io::ErrorKind::NotFound, "test"));
        let diags = err.to_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
    }

    #[test]
    fn test_compiler_error_to_diagnostics_file_not_found() {
        let err = CompilerError::FileNotFound("test.mi".to_string());
        let diags = err.to_diagnostics();
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].severity, Severity::Error);
        assert!(diags[0].message.contains("test.mi"));
    }

    #[test]
    fn test_compiler_error_to_diagnostics_internal() {
        let err = CompilerError::Internal("something went wrong".to_string());
        let diags = err.to_diagnostics();
        assert_eq!(diags.len(), 1);
        assert!(diags[0].message.contains("something went wrong"));
    }

    #[test]
    fn test_json_diagnostics_envelope_roundtrip() {
        use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};
        use serde_json;

        let envelope = DiagnosticsEnvelope {
            schema_version: 1,
            ok: true,
            command: JsonCommand::Check,
            diagnostics: vec![],
            artifact: None,
            exit_code: None,
            stdout_tail: None,
            stderr_tail: None,
            stdout_truncated: None,
            stderr_truncated: None,
            duration_ms: None,
            tests: None,
            explanation: None,
        };

        let json_str = serde_json::to_string(&envelope).expect("serialize");
        let parsed: DiagnosticsEnvelope = serde_json::from_str(&json_str).expect("deserialize");

        assert_eq!(parsed.schema_version, 1);
        assert!(parsed.ok);
        assert_eq!(parsed.command, JsonCommand::Check);
        assert!(parsed.diagnostics.is_empty());
        assert!(parsed.artifact.is_none());
    }

    #[test]
    fn test_json_diagnostic_with_all_fields() {
        use miri::diagnostics::json::JsonDiagnostic;
        use serde_json;

        let diag = JsonDiagnostic {
            severity: "error".to_string(),
            code: Some("MER_TYP_010".to_string()),
            message: "type mismatch".to_string(),
            path: Some("test.mi".to_string()),
            line: Some(5),
            column: Some(10),
            length: Some(3),
            expected: Some("Int".to_string()),
            actual: Some("Bool".to_string()),
            help: Some("add explicit type cast".to_string()),
            fix_safety: None,
            repair: None,
            related: vec![],
        };

        let json_str = serde_json::to_string(&diag).expect("serialize");
        assert!(json_str.contains("error"));
        assert!(json_str.contains("MER_TYP_010"));
        assert!(json_str.contains("\"type mismatch\""));
    }

    #[test]
    fn test_diagnostic_to_json() {
        use miri::error::diagnostic::to_json;
        use miri::error::syntax::Span;

        let source = "fn main() int\n    x + y\n";
        let span = Span { start: 20, end: 25 };

        let diag = DiagnosticBuilder::error("Type Mismatch")
            .code("MER_TYP_010")
            .message("Expected Int but got Bool")
            .expected("Int")
            .actual("Bool")
            .span(span)
            .help("add a cast")
            .add_note("see the reference")
            .build();

        let json_diag = to_json(&diag, source, Some("test.mi"));

        assert_eq!(json_diag.severity, "error");
        assert_eq!(json_diag.code, Some("MER_TYP_010".to_string()));
        assert_eq!(json_diag.message, "Expected Int but got Bool");
        assert_eq!(json_diag.expected, Some("Int".to_string()));
        assert_eq!(json_diag.actual, Some("Bool".to_string()));
        assert_eq!(json_diag.path, Some("test.mi".to_string()));
        assert_eq!(json_diag.help, Some("add a cast".to_string()));
        assert_eq!(json_diag.related.len(), 1);
        assert_eq!(json_diag.related[0].message, "see the reference");
    }
}
