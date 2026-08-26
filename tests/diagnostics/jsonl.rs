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
