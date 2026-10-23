// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for the line shapes of the watch stream.
//!
//! What is pinned here is the published format: that a framing line survives a
//! round trip unchanged, that it refuses a member it does not know, that a
//! framing line and a diagnostic line cannot be mistaken for one another, and
//! that a batch written to a stream is a sequence of whole JSON objects.

use miri::diagnostics::json::{JsonDiagnostic, SCHEMA_VERSION};
use miri::diagnostics::jsonl::{write_diagnostic, write_event, DevEvent, DevStreamLine};

/// A diagnostic with only the members every diagnostic has.
fn diagnostic(message: &str) -> JsonDiagnostic {
    JsonDiagnostic {
        severity: "error".to_string(),
        code: Some("MER_TYP_010".to_string()),
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
    }
}

/// Serialize `event`, read it back, and return both halves.
fn round_trip(event: &DevEvent) -> (String, DevEvent) {
    let line = serde_json::to_string(event).expect("a framing line should serialize");
    let parsed = serde_json::from_str(&line).expect("a framing line should parse");
    (line, parsed)
}

#[test]
fn a_tick_survives_a_round_trip() {
    let opening = DevEvent::tick(0, "/project/main.mi");
    let (_, parsed) = round_trip(&opening);
    assert_eq!(opening, parsed);
}

#[test]
fn an_idle_survives_a_round_trip() {
    let closing = DevEvent::idle(false, 12);
    let (_, parsed) = round_trip(&closing);
    assert_eq!(closing, parsed);
}

#[test]
fn a_tick_carries_the_schema_version() {
    let (line, _) = round_trip(&DevEvent::tick(0, "/project/main.mi"));
    assert!(
        line.contains(&format!(r#""schemaVersion":{}"#, SCHEMA_VERSION)),
        "the opening line should state the schema version: {}",
        line
    );
}

#[test]
fn the_first_batch_of_a_session_opens_at_zero() {
    let (line, _) = round_trip(&DevEvent::tick(0, "/project/main.mi"));
    assert!(
        line.contains(r#""ts":0"#),
        "a session's first batch opens at zero: {}",
        line
    );
}

#[test]
fn a_framing_line_is_written_in_camel_case() {
    let (line, _) = round_trip(&DevEvent::idle(true, 7));
    assert!(
        line.contains(r#""durationMs":7"#),
        "the closing line should use camelCase: {}",
        line
    );
}

#[test]
fn a_tick_refuses_a_member_it_does_not_know() {
    let line = r#"{"event":"tick","schemaVersion":1,"ts":0,"path":"/m.mi","extra":1}"#;
    assert!(
        serde_json::from_str::<DevEvent>(line).is_err(),
        "an unknown member should be refused, not ignored"
    );
}

#[test]
fn an_idle_refuses_a_member_it_does_not_know() {
    let line = r#"{"event":"idle","ok":true,"durationMs":7,"extra":1}"#;
    assert!(
        serde_json::from_str::<DevEvent>(line).is_err(),
        "an unknown member should be refused, not ignored"
    );
}

#[test]
fn an_unrecognised_event_name_is_refused() {
    let line = r#"{"event":"settled","ok":true,"durationMs":7}"#;
    assert!(
        serde_json::from_str::<DevEvent>(line).is_err(),
        "only the published event names should parse"
    );
}

#[test]
fn a_framing_line_is_not_a_diagnostic() {
    let line = serde_json::to_string(&DevEvent::tick(0, "/project/main.mi"))
        .expect("a framing line should serialize");
    assert!(
        serde_json::from_str::<JsonDiagnostic>(&line).is_err(),
        "a framing line must not be readable as a diagnostic"
    );
}

#[test]
fn a_diagnostic_is_not_a_framing_line() {
    let line =
        serde_json::to_string(&diagnostic("type mismatch")).expect("a diagnostic should serialize");
    assert!(
        serde_json::from_str::<DevEvent>(&line).is_err(),
        "a diagnostic must not be readable as a framing line"
    );
}

#[test]
fn the_reader_tells_the_three_line_kinds_apart() {
    let opening = serde_json::to_string(&DevEvent::tick(3, "/project/main.mi"))
        .expect("a framing line should serialize");
    let middle =
        serde_json::to_string(&diagnostic("type mismatch")).expect("a diagnostic should serialize");
    let closing =
        serde_json::to_string(&DevEvent::idle(false, 12)).expect("a framing line should serialize");

    match DevStreamLine::parse(&opening) {
        Ok(DevStreamLine::Event(DevEvent::Tick { ts, path, .. })) => {
            assert_eq!(ts, 3);
            assert_eq!(path, "/project/main.mi");
        }
        other => panic!("the opening line should read as a tick, got {:?}", other),
    }

    match DevStreamLine::parse(&middle) {
        Ok(DevStreamLine::Diagnostic(read)) => assert_eq!(read.message, "type mismatch"),
        other => panic!(
            "the middle line should read as a diagnostic, got {:?}",
            other
        ),
    }

    match DevStreamLine::parse(&closing) {
        Ok(DevStreamLine::Event(DevEvent::Idle { ok, duration_ms })) => {
            assert!(!ok);
            assert_eq!(duration_ms, 12);
        }
        other => panic!("the closing line should read as an idle, got {:?}", other),
    }
}

#[test]
fn a_batch_that_found_nothing_is_two_whole_objects() {
    let mut stream = Vec::new();
    write_event(&mut stream, &DevEvent::tick(0, "/project/main.mi"))
        .expect("the stream should accept the opening line");
    write_event(&mut stream, &DevEvent::idle(true, 5))
        .expect("the stream should accept the closing line");

    let lines = whole_lines(&stream);
    assert_eq!(lines.len(), 2);
    assert!(matches!(
        DevStreamLine::parse(&lines[0]),
        Ok(DevStreamLine::Event(DevEvent::Tick { .. }))
    ));
    assert!(matches!(
        DevStreamLine::parse(&lines[1]),
        Ok(DevStreamLine::Event(DevEvent::Idle { .. }))
    ));
}

#[test]
fn a_batch_carries_one_whole_object_per_diagnostic() {
    let mut stream = Vec::new();
    write_event(&mut stream, &DevEvent::tick(10, "/project/main.mi"))
        .expect("the stream should accept the opening line");
    write_diagnostic(&mut stream, &diagnostic("first"))
        .expect("the stream should accept a diagnostic");
    write_diagnostic(&mut stream, &diagnostic("second"))
        .expect("the stream should accept a diagnostic");
    write_event(&mut stream, &DevEvent::idle(false, 12))
        .expect("the stream should accept the closing line");

    let lines = whole_lines(&stream);
    assert_eq!(lines.len(), 4);

    let messages: Vec<String> = lines[1..3]
        .iter()
        .filter_map(|line| match DevStreamLine::parse(line) {
            Ok(DevStreamLine::Diagnostic(read)) => Some(read.message),
            _ => None,
        })
        .collect();
    assert_eq!(messages, vec!["first".to_string(), "second".to_string()]);
}

#[test]
fn a_batch_carries_diagnostics_of_differing_severities() {
    let severities = ["error", "warning", "note"];

    let mut stream = Vec::new();
    write_event(&mut stream, &DevEvent::tick(0, "/project/main.mi"))
        .expect("the stream should accept the opening line");
    for severity in severities {
        let mut carried = diagnostic("something to say");
        carried.severity = severity.to_string();
        write_diagnostic(&mut stream, &carried).expect("the stream should accept a diagnostic");
    }
    write_event(&mut stream, &DevEvent::idle(true, 3))
        .expect("the stream should accept the closing line");

    let lines = whole_lines(&stream);
    assert_eq!(lines.len(), 5);

    let read: Vec<String> = lines[1..4]
        .iter()
        .filter_map(|line| match DevStreamLine::parse(line) {
            Ok(DevStreamLine::Diagnostic(diagnostic)) => Some(diagnostic.severity),
            _ => None,
        })
        .collect();
    assert_eq!(read, severities, "every severity should survive the batch");
}

#[test]
fn text_inside_a_diagnostic_cannot_forge_a_framing_line() {
    // A diagnostic quotes back identifiers and literals from the source, so its
    // message carries text the author of the file chose. Were it written raw, a
    // newline in that text would split one line into two and the second could
    // be read as a batch closing early.
    let forged = "oops\n{\"event\":\"idle\",\"ok\":true,\"durationMs\":0}";
    let mut stream = Vec::new();
    write_event(&mut stream, &DevEvent::tick(0, "/project/main.mi"))
        .expect("the stream should accept the opening line");
    write_diagnostic(&mut stream, &diagnostic(forged))
        .expect("the stream should accept a diagnostic");
    write_event(&mut stream, &DevEvent::idle(false, 1))
        .expect("the stream should accept the closing line");

    let lines = whole_lines(&stream);
    assert_eq!(
        lines.len(),
        3,
        "the text should stay inside its own line, not become one"
    );

    match DevStreamLine::parse(&lines[1]) {
        Ok(DevStreamLine::Diagnostic(read)) => assert_eq!(
            read.message, forged,
            "the text should survive the round trip unchanged"
        ),
        other => panic!(
            "the middle line should still be a diagnostic, got {:?}",
            other
        ),
    }
}

#[test]
fn a_path_holding_a_newline_stays_one_framing_line() {
    let mut stream = Vec::new();
    write_event(&mut stream, &DevEvent::tick(0, "/project/ev\nil.mi"))
        .expect("the stream should accept the opening line");

    let lines = whole_lines(&stream);
    assert_eq!(lines.len(), 1, "a path cannot split its own line");
}

#[test]
fn parse_fails_on_invalid_json_syntax() {
    let invalid_jsons = ["{", "not json", r#"{"event": "tick", "path": }"#];
    for line in invalid_jsons {
        assert!(
            DevStreamLine::parse(line).is_err(),
            "invalid JSON syntax should fail to parse: {}",
            line
        );
    }
}

#[test]
fn parse_fails_on_missing_required_framing_fields() {
    // Missing ts or path in tick
    let tick_missing_ts = r#"{"event":"tick","schemaVersion":1,"path":"/m.mi"}"#;
    assert!(
        DevStreamLine::parse(tick_missing_ts).is_err(),
        "tick event missing 'ts' should fail to parse"
    );

    let tick_missing_path = r#"{"event":"tick","schemaVersion":1,"ts":0}"#;
    assert!(
        DevStreamLine::parse(tick_missing_path).is_err(),
        "tick event missing 'path' should fail to parse"
    );

    // Missing ok or durationMs in idle
    let idle_missing_ok = r#"{"event":"idle","durationMs":10}"#;
    assert!(
        DevStreamLine::parse(idle_missing_ok).is_err(),
        "idle event missing 'ok' should fail to parse"
    );

    let idle_missing_duration = r#"{"event":"idle","ok":true}"#;
    assert!(
        DevStreamLine::parse(idle_missing_duration).is_err(),
        "idle event missing 'durationMs' should fail to parse"
    );
}

#[test]
fn parse_fails_on_missing_required_diagnostic_fields() {
    // Missing severity
    let missing_severity = r#"{"message":"something went wrong"}"#;
    assert!(
        DevStreamLine::parse(missing_severity).is_err(),
        "diagnostic missing 'severity' should fail to parse"
    );

    // Missing message
    let missing_message = r#"{"severity":"error"}"#;
    assert!(
        DevStreamLine::parse(missing_message).is_err(),
        "diagnostic missing 'message' should fail to parse"
    );
}

#[test]
fn framing_lines_and_diagnostics_support_numeric_and_empty_boundaries() {
    // u64::MAX timestamp and duration, empty path and empty message
    let max_tick = DevEvent::tick(u64::MAX, "");
    let (_, parsed_tick) = round_trip(&max_tick);
    assert_eq!(max_tick, parsed_tick);

    let max_idle = DevEvent::idle(true, u64::MAX);
    let (_, parsed_idle) = round_trip(&max_idle);
    assert_eq!(max_idle, parsed_idle);

    let empty_diag = diagnostic("");
    let line = serde_json::to_string(&empty_diag).expect("serialization should succeed");
    match DevStreamLine::parse(&line) {
        Ok(DevStreamLine::Diagnostic(d)) => assert_eq!(d.message, ""),
        other => panic!("expected Diagnostic with empty message, got {:?}", other),
    }
}

#[test]
fn write_event_and_write_diagnostic_propagate_io_errors() {
    struct FailingWriter;
    impl std::io::Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "write error",
            ))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "flush error",
            ))
        }
    }

    let mut writer = FailingWriter;
    let tick = DevEvent::tick(0, "/project/main.mi");
    assert!(
        write_event(&mut writer, &tick).is_err(),
        "write_event should propagate I/O write error"
    );

    let diag = diagnostic("test error");
    assert!(
        write_diagnostic(&mut writer, &diag).is_err(),
        "write_diagnostic should propagate I/O write error"
    );
}

#[test]
fn parse_handles_diagnostic_with_full_optional_fields_and_related_notes() {
    use miri::diagnostics::json::{JsonEdit, JsonRelated, JsonRepair};

    let full_diag = JsonDiagnostic {
        severity: "error".to_string(),
        code: Some("MER_TYP_010".to_string()),
        message: "mismatched types".to_string(),
        path: Some("src/main.mi".to_string()),
        line: Some(42),
        column: Some(15),
        length: Some(4),
        expected: Some("Int".to_string()),
        actual: Some("String".to_string()),
        help: Some("convert string to int".to_string()),
        fix_safety: Some("behavior-preserving".to_string()),
        repair: Some(JsonRepair {
            id: "fix_type".to_string(),
            summary: "parse string as int".to_string(),
            edits: vec![JsonEdit {
                path: "src/main.mi".to_string(),
                start: 100,
                end: 105,
                replacement: "42".to_string(),
            }],
        }),
        related: vec![JsonRelated {
            severity: "note".to_string(),
            message: "defined here".to_string(),
            code: None,
            path: Some("src/main.mi".to_string()),
            line: Some(10),
            column: Some(1),
        }],
        preexisting: Some(false),
    };

    let json_str =
        serde_json::to_string(&full_diag).expect("diagnostic serialization should succeed");
    match DevStreamLine::parse(&json_str) {
        Ok(DevStreamLine::Diagnostic(d)) => {
            assert_eq!(*d, full_diag);
            assert_eq!(d.related.len(), 1);
            assert_eq!(d.related[0].message, "defined here");
            assert!(d.repair.is_some());
            assert_eq!(d.repair.as_ref().unwrap().id, "fix_type");
        }
        other => panic!("expected Diagnostic, got {:?}", other),
    }
}

/// Split a written stream into lines, asserting each is one whole JSON object.
///
/// This is the property a consumer reading the stream a line at a time depends
/// on: no line is ever a fragment of an object.
fn whole_lines(stream: &[u8]) -> Vec<String> {
    let text = String::from_utf8(stream.to_vec()).expect("the stream should be UTF-8");
    assert!(
        text.ends_with('\n'),
        "every line, the last one included, should be terminated"
    );
    text.lines()
        .map(|line| {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|error| panic!("a line should be one whole object: {}", error));
            line.to_string()
        })
        .collect()
}
