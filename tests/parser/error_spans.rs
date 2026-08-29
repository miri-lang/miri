// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::error::syntax::{find_line_info, SyntaxErrorKind};
use miri::lexer::Lexer;
use miri::parser::Parser;
use std::fs;

fn spanned_text(source: &str, span: miri::error::Span) -> &str {
    &source[span.start..span.end]
}

#[test]
fn test_error_span_selects_exactly_the_offending_token() {
    struct TestCase {
        label: &'static str,
        source: &'static str,
        expected_token: &'static str,
        expected_line: usize,
        expected_column: usize,
    }

    let test_cases = vec![
        TestCase {
            label: "type_annotation_colon",
            source: "let x: int = 1\n",
            expected_token: ":",
            expected_line: 1,
            expected_column: 6,
        },
        TestCase {
            label: "return_type_colon_in_function",
            source: "use system.io\n\nfn main() int:\n    let x: int = 1\n    println(\"hi\")\n    return 0\n",
            expected_token: ":",
            expected_line: 4,
            expected_column: 10,
        },
        TestCase {
            label: "unexpected_brace_in_if_statement",
            source: "use system.io\n\nfn main() int:\n    var i = 0\n    if i == 0 {\n        println(\"yes\")\n    }\n    return 0\n",
            expected_token: "{",
            expected_line: 5,
            expected_column: 15,
        },
        TestCase {
            label: "arrow_operator_in_function_signature",
            source: "use system.io\n\nfn add(a int, b int) -> int:\n    return a + b\n\nfn main() int:\n    println(\"x\")\n    return 0\n",
            expected_token: "->",
            expected_line: 3,
            expected_column: 22,
        },
        TestCase {
            label: "paren_in_for_loop_pattern",
            source: "use system.io\n\nfn main() int:\n    for (k, v) in m:\n        println(\"x\")\n    return 0\n",
            expected_token: "(",
            expected_line: 4,
            expected_column: 9,
        },
        TestCase {
            label: "identifier_in_elif_condition",
            source: "use system.io\n\nfn main() int:\n    if x:\n        return 1\n    elif x:\n        return 2\n",
            expected_token: "x",
            expected_line: 6,
            expected_column: 10,
        },
        TestCase {
            label: "class_name_in_impl",
            source: "impl Foo:\n    fn a()\n",
            expected_token: "Foo",
            expected_line: 1,
            expected_column: 6,
        },
    ];

    for case in test_cases {
        let error = {
            let mut lexer = Lexer::new(case.source);
            let mut parser = Parser::new(&mut lexer, case.source);
            parser.parse().unwrap_err()
        };

        let actual_token = spanned_text(case.source, error.span);
        assert_eq!(
            actual_token, case.expected_token,
            "case '{}': expected token '{}' but got '{}'",
            case.label, case.expected_token, actual_token
        );

        let (actual_line, actual_column, _) = find_line_info(case.source, error.span.start);
        assert_eq!(
            actual_line, case.expected_line,
            "case '{}': expected line {} but got {}",
            case.label, case.expected_line, actual_line
        );
        assert_eq!(
            actual_column, case.expected_column,
            "case '{}': expected column {} but got {}",
            case.label, case.expected_column, actual_column
        );
    }
}

/// Validate exact error spans for specific rejection fixtures.
///
/// These fixtures require specific error anchoring by construct (e.g. the `match`
/// keyword for missing branches, or the duplicate pattern itself for pattern
/// duplicates). This table documents the exact expected span for each fixture.
#[test]
fn test_fixture_error_spans_are_exactly_anchored() {
    struct FixtureCase {
        filename: &'static str,
        expected_token: &'static str,
        expected_line: usize,
        expected_column: usize,
    }

    let cases = vec![
        FixtureCase {
            filename: "MER_PAR_010.mi",
            expected_token: "1",
            expected_line: 6,
            expected_column: 9,
        },
        FixtureCase {
            filename: "MER_PAR_011.mi",
            expected_token: "match",
            expected_line: 4,
            expected_column: 5,
        },
        FixtureCase {
            filename: "MER_PAR_012.mi",
            expected_token: "x",
            expected_line: 4,
            expected_column: 5,
        },
        FixtureCase {
            filename: "MER_PAR_013.mi",
            expected_token: "Empty",
            expected_line: 3,
            expected_column: 8,
        },
        FixtureCase {
            filename: "MER_PAR_014.mi",
            expected_token: "Empty",
            expected_line: 3,
            expected_column: 6,
        },
    ];

    for case in cases {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("conformance/agent/fail")
            .join(case.filename);
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|_| panic!("fixture {} must be readable", case.filename));

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        let error = match parser.parse() {
            Err(e) => e,
            Ok(_) => panic!("fixture {} must fail to parse", case.filename),
        };

        let actual_token = spanned_text(&source, error.span);
        assert_eq!(
            actual_token, case.expected_token,
            "fixture '{}': expected token '{}' but got '{}'",
            case.filename, case.expected_token, actual_token
        );

        let (actual_line, actual_column, _) = find_line_info(&source, error.span.start);
        assert_eq!(
            actual_line, case.expected_line,
            "fixture '{}': expected line {} but got {}",
            case.filename, case.expected_line, actual_line
        );
        assert_eq!(
            actual_column, case.expected_column,
            "fixture '{}': expected column {} but got {}",
            case.filename, case.expected_column, actual_column
        );
    }
}

/// The offending token for every published rejection fixture.
///
/// A hand-written table only covers the cases someone thought to write down.
/// Sweeping the fixture corpus means a rejection added tomorrow is held to the
/// same rule the day it lands.
#[test]
fn test_parse_error_spans_point_inside_the_source() {
    // The parser is recursive descent and the corpus deliberately holds a
    // deeply nested expression. A test thread gets a smaller stack than the
    // compiler binary runs on, so the sweep takes a stack of its own rather
    // than dropping the fixture that needs one.
    let sweep = std::thread::Builder::new()
        .stack_size(16 * 1024 * 1024)
        .spawn(sweep_rejection_fixtures)
        .expect("the sweep thread must start");
    if let Err(panic) = sweep.join() {
        std::panic::resume_unwind(panic);
    }
}

/// Check every rejection fixture, collecting each misplaced span before failing.
fn sweep_rejection_fixtures() {
    let fixtures = rejection_fixtures();
    assert!(
        !fixtures.is_empty(),
        "no MER_PAR_*.mi fixtures were found under conformance/agent/fail"
    );

    let mut rejected = 0;
    let mut failures = Vec::new();

    for path in &fixtures {
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let source = fs::read_to_string(path).expect("a fixture must be readable");

        let mut lexer = Lexer::new(&source);
        let mut parser = Parser::new(&mut lexer, &source);
        // A fixture the parser accepts is rejected by a later stage; only the
        // ones that fail here carry a span this rule can judge.
        let Err(error) = parser.parse() else {
            continue;
        };
        rejected += 1;

        if let Some(reason) = misplaced_span_reason(&source, &error) {
            failures.push(format!("{}: {} [{:?}]", name, reason, error.kind));
        }
    }

    assert!(
        rejected > 0,
        "{} fixtures were read but none reached the parser",
        fixtures.len()
    );
    assert!(
        failures.is_empty(),
        "{} of {} rejected fixtures anchor their error outside the source:\n{}",
        failures.len(),
        rejected,
        failures.join("\n")
    );
}

/// Why `error` fails to name real bytes of `source`, or `None` when it does.
///
/// An exhausted token stream is the one case with nothing left to point at, so
/// `UnexpectedEOF` alone may sit at the end of the source.
fn misplaced_span_reason(source: &str, error: &miri::error::SyntaxError) -> Option<String> {
    if error.kind == SyntaxErrorKind::UnexpectedEOF {
        return None;
    }
    if error.span.end <= error.span.start {
        return Some(format!(
            "empty span at byte {}, so there is no token to underline",
            error.span.start
        ));
    }
    if error.span.start >= source.len() {
        return Some(format!(
            "span starts at byte {} but the source ends at {}",
            error.span.start,
            source.len()
        ));
    }
    let (line, column, _) = find_line_info(source, error.span.start);
    let last_line = source.lines().count();
    if line > last_line {
        return Some(format!(
            "reported at {}:{} but the source ends on line {}",
            line, column, last_line
        ));
    }
    None
}

/// Every published parser-rejection fixture, in a stable order.
fn rejection_fixtures() -> Vec<std::path::PathBuf> {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("conformance/agent/fail");
    let entries = fs::read_dir(&dir).expect("the rejection fixture directory must exist");
    let mut paths: Vec<std::path::PathBuf> = entries
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("MER_PAR_") && name.ends_with(".mi"))
        })
        .collect();
    paths.sort();
    paths
}
