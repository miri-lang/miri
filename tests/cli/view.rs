// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri view` — scoped source reads.

use std::io::Write;
use std::path::Path;

use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};

use crate::utils::miri_cmd;

/// A program with two containers declaring a method of the same name, a nested
/// loop to anchor into, and a doc comment to surface in an outline.
const PROBE: &str = "\
// Adds the positive values.
fn total(values [int]) int
    var sum = 0
    for v in values
        if v > 0
            sum = sum + v
    return sum

class Point
    public x int

    // Moves the point.
    fn shift(d int) int
        return self.x + d

class Line
    fn shift(d int) int
        return d
";

/// Write `source` to a temporary file and hand it to `body`.
fn with_source<T>(source: &str, body: impl FnOnce(&Path) -> T) -> T {
    let mut file = tempfile::Builder::new()
        .suffix(".mi")
        .tempfile()
        .expect("a temporary source file can be created");
    file.write_all(source.as_bytes())
        .expect("the fixture can be written");
    file.flush().expect("the fixture reaches disk");
    body(file.path())
}

/// Run `miri view` and return (stdout, stderr, success).
fn view(args: &[&str]) -> (String, String, bool) {
    let output = miri_cmd()
        .arg("view")
        .args(args)
        .output()
        .expect("the view command runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Parse the envelope out of a JSON run.
fn envelope(stdout: &str) -> DiagnosticsEnvelope {
    serde_json::from_str(stdout).expect("view emits a parseable envelope")
}

#[test]
fn test_fn_prints_only_the_named_function() {
    let (stdout, _, ok) = view(&["--fn", "main", "examples/hello.mi"]);
    assert!(ok, "viewing main should succeed");
    assert!(stdout.contains("fn main()"), "got: {stdout}");
    assert!(
        stdout.contains("println(\"Hello, World!\")"),
        "the body should be shown: {stdout}"
    );
    assert!(
        !stdout.contains("use system.io"),
        "nothing but the function should be printed: {stdout}"
    );
    assert!(
        !stdout.contains("SPDX"),
        "the file header should not be printed: {stdout}"
    );
}

#[test]
fn test_fn_resolves_a_method_by_its_container() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&["--fn", "Point.shift", &path.display().to_string()]);
        assert!(ok, "viewing a method should succeed");
        assert!(stdout.contains("return self.x + d"), "got: {stdout}");
        assert!(
            !stdout.contains("return d\n"),
            "the other container's method should not appear: {stdout}"
        );
    });
}

#[test]
fn test_fn_that_does_not_exist_reports_its_code() {
    with_source(PROBE, |path| {
        let (stdout, stderr, ok) = view(&[
            "--fn",
            "missing",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(!ok, "a missing function should fail");
        let envelope = envelope(&stdout);
        assert_eq!(envelope.command, JsonCommand::View);
        assert!(!envelope.ok, "the envelope should report failure");
        assert_eq!(
            envelope.diagnostics[0].code.as_deref(),
            Some("MER_BLD_004"),
            "stderr was: {stderr}"
        );
    });
}

#[test]
fn test_a_name_two_containers_declare_is_ambiguous() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "shift",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(!ok, "an ambiguous name should fail");
        let envelope = envelope(&stdout);
        assert_eq!(envelope.diagnostics[0].code.as_deref(), Some("MER_BLD_005"));
        let message = &envelope.diagnostics[0].message;
        assert!(
            message.contains("Point.shift") && message.contains("Line.shift"),
            "the candidates should be named: {message}"
        );
    });
}

#[test]
fn test_around_narrows_to_the_innermost_block() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "total",
            "--around",
            "sum = sum + v",
            &path.display().to_string(),
        ]);
        assert!(ok, "narrowing should succeed");
        assert!(stdout.contains("sum = sum + v"), "got: {stdout}");
        assert!(
            !stdout.contains("fn total"),
            "the signature is outside the innermost block: {stdout}"
        );
        assert!(
            !stdout.contains("return sum"),
            "statements outside the block should be dropped: {stdout}"
        );
    });
}

#[test]
fn test_around_text_that_is_absent_reports_its_code() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "total",
            "--around",
            "no such text",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(!ok, "an absent anchor should fail");
        assert_eq!(
            envelope(&stdout).diagnostics[0].code.as_deref(),
            Some("MER_BLD_006")
        );
    });
}

#[test]
fn test_around_text_that_repeats_reports_its_code() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "total",
            "--around",
            "sum",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(!ok, "a repeated anchor should fail");
        let envelope = envelope(&stdout);
        assert_eq!(envelope.diagnostics[0].code.as_deref(), Some("MER_BLD_007"));
        assert!(
            envelope.diagnostics[0].message.contains("occurs"),
            "the count should be reported: {}",
            envelope.diagnostics[0].message
        );
    });
}

#[test]
fn test_outline_lists_signatures_and_doc_lines_without_bodies() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&["--outline", &path.display().to_string()]);
        assert!(ok, "an outline should succeed");
        assert!(
            stdout.contains("fn total(values [int]) int"),
            "got: {stdout}"
        );
        assert!(
            stdout.contains("// Adds the positive values."),
            "got: {stdout}"
        );
        assert!(stdout.contains("class Point"), "got: {stdout}");
        assert!(
            stdout.contains("    fn shift(d int) int"),
            "members should be listed under their container: {stdout}"
        );
        assert!(
            !stdout.contains("sum = sum + v"),
            "an outline carries no bodies: {stdout}"
        );
        assert!(
            !stdout.contains("return self.x + d"),
            "an outline carries no bodies: {stdout}"
        );
    });
}

#[test]
fn test_outline_is_a_fraction_of_the_module_it_summarizes() {
    let module = Path::new("src/stdlib/system/json.mi");
    let source = std::fs::read_to_string(module).expect("the module is readable");
    assert!(
        source.lines().count() > 500,
        "this assertion is about a large module"
    );

    let (stdout, _, ok) = view(&["--outline", &module.display().to_string()]);
    assert!(ok, "an outline of the module should succeed");

    // The spec estimated 10%; the outline measures ~18% because it lists
    // methods (the grammar counts a function inside a class as a declaration)
    // and carries each declaration's doc line, both of which the same criterion
    // asks for. The bound is set just above the measured value so a regression
    // that inflates the outline fails here rather than passing quietly.
    let ratio = stdout.len() as f64 / source.len() as f64;
    assert!(
        ratio < 0.20,
        "an outline should be a small fraction of its module, was {:.1}% ({} of {} bytes)",
        ratio * 100.0,
        stdout.len(),
        source.len()
    );
}

#[test]
fn test_outline_does_not_invent_a_main_for_a_script() {
    // A file of top-level statements gets a synthetic `main` when the compiler
    // builds it. A read must show what was written, not what the compiler adds.
    with_source("let x = 1\nprintln(x)\n", |path| {
        let (stdout, _, ok) = view(&["--outline", &path.display().to_string()]);
        assert!(ok, "an outline of a script should succeed");
        assert!(
            !stdout.contains("main"),
            "no main was written, so none should be listed: {stdout}"
        );
    });
}

#[test]
fn test_json_spans_delimit_the_declarations_in_the_text() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "total",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(ok, "viewing should succeed");
        let envelope = envelope(&stdout);
        let view = envelope.view.expect("a successful view carries its text");
        assert_eq!(view.shape, "fn");
        assert!(!view.spans.is_empty(), "a function view records its span");

        for span in &view.spans {
            let slice = &view.text[span.start..span.end];
            let name = span.name.clone().unwrap_or_default();
            assert!(
                slice.starts_with(&format!("fn {name}")),
                "span {span:?} does not delimit its declaration: {slice:?}"
            );
        }
    });
}

#[test]
fn test_outline_json_reports_its_shape_and_every_declaration() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&["--outline", &path.display().to_string(), "--format", "json"]);
        assert!(ok, "an outline should succeed");
        let view = envelope(&stdout)
            .view
            .expect("a successful outline carries its text");
        assert_eq!(view.shape, "outline");

        let names: Vec<_> = view
            .spans
            .iter()
            .filter_map(|span| span.name.clone())
            .collect();
        assert!(names.contains(&"total".to_string()), "got: {names:?}");
        assert!(names.contains(&"Point".to_string()), "got: {names:?}");
        assert!(names.contains(&"shift".to_string()), "got: {names:?}");

        for span in &view.spans {
            let slice = view
                .text
                .get(span.start..span.end)
                .unwrap_or_else(|| panic!("span {span:?} should index the text it came with"));
            let name = span.name.clone().unwrap_or_default();
            assert!(
                slice.contains(&name),
                "span {span:?} does not delimit its declaration: {slice:?}"
            );
            assert!(
                !slice.starts_with(' ') && !slice.contains('\n'),
                "a signature span should cover one declaration head: {slice:?}"
            );
        }
    });
}

#[test]
fn test_around_narrows_inside_a_forall_body() {
    // Narrowing walks every statement that owns a body, not just the loops the
    // other tests cover.
    let source = "\
const N = 4

fn run(dst [int], a [int])
    forall i in 0..N
        dst[i] = a[i] + 1
";
    with_source(source, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "run",
            "--around",
            "dst[i] = a[i] + 1",
            &path.display().to_string(),
        ]);
        assert!(ok, "narrowing into a forall body should succeed");
        assert!(stdout.contains("dst[i] = a[i] + 1"), "got: {stdout}");
        assert!(
            !stdout.contains("fn run"),
            "only the innermost block should be shown: {stdout}"
        );
    });
}

#[test]
fn test_an_unreadable_file_still_answers_in_the_requested_format() {
    let (stdout, _, ok) = view(&[
        "--fn",
        "main",
        "/nonexistent/does_not_exist.mi",
        "--format",
        "json",
    ]);
    assert!(!ok, "an unreadable file should fail");
    let envelope = envelope(&stdout);
    assert!(!envelope.ok, "the envelope should report failure");
    assert_eq!(
        envelope.diagnostics[0].code.as_deref(),
        Some("MER_BLD_008"),
        "a JSON request must be answered with an envelope, not prose"
    );
}

#[test]
fn test_view_does_not_echo_terminal_escapes_from_an_anchor() {
    with_source(PROBE, |path| {
        let (_, stderr, ok) = view(&[
            "--fn",
            "total",
            "--around",
            "a\u{1b}[31mb",
            &path.display().to_string(),
        ]);
        assert!(!ok, "an absent anchor should fail");
        assert!(
            !stderr.contains('\u{1b}'),
            "a crafted anchor must not repaint the terminal: {stderr:?}"
        );
    });
}

#[test]
fn test_fn_and_outline_cannot_be_asked_for_together() {
    with_source(PROBE, |path| {
        let (_, stderr, ok) = view(&["--fn", "total", "--outline", &path.display().to_string()]);
        assert!(!ok, "the two shapes are mutually exclusive");
        assert!(
            stderr.contains("cannot be used with"),
            "clap should reject the combination: {stderr}"
        );
    });
}

#[test]
fn test_around_requires_a_function_to_narrow() {
    with_source(PROBE, |path| {
        let (_, stderr, ok) = view(&["--around", "sum", &path.display().to_string()]);
        assert!(!ok, "an anchor without a function should be rejected");
        assert!(
            stderr.contains("--fn") || stderr.contains("required"),
            "clap should say what is missing: {stderr}"
        );
    });
}

#[test]
fn test_a_shape_must_be_asked_for() {
    with_source(PROBE, |path| {
        let (_, stderr, ok) = view(&[&path.display().to_string()]);
        assert!(!ok, "a shapeless request should be rejected");
        assert!(
            stderr.contains("required") || stderr.contains("--fn"),
            "clap should say what is missing: {stderr}"
        );
    });
}

#[test]
fn test_a_file_that_does_not_parse_is_reported_not_crashed() {
    with_source("fn main(\n", |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "main",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(!ok, "unparseable source should fail");
        let envelope = envelope(&stdout);
        assert!(!envelope.ok);
        assert!(
            !envelope.diagnostics.is_empty(),
            "the parse error should be reported"
        );
    });
}

#[test]
fn test_view_does_not_echo_terminal_escapes_from_its_argument() {
    with_source(PROBE, |path| {
        let (_, stderr, ok) = view(&["--fn", "a\u{1b}[31mb", &path.display().to_string()]);
        assert!(!ok, "an unknown name should fail");
        assert!(
            !stderr.contains('\u{1b}'),
            "a crafted name must not repaint the terminal: {stderr:?}"
        );
    });
}

#[test]
fn test_the_rendered_text_is_the_same_through_the_agent() {
    with_source(PROBE, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "Point.shift",
            &path.display().to_string(),
            "--format",
            "json",
        ]);
        assert!(ok, "the command should succeed");
        let from_command = envelope(&stdout)
            .view
            .expect("a successful view carries its text")
            .text;

        let from_agent = agent_view(path, "Point.shift");
        assert_eq!(
            from_command, from_agent,
            "a request over the connection should read back the same text"
        );
    });
}

#[test]
fn test_the_agent_rejects_an_anchor_with_no_function() {
    with_source(PROBE, |path| {
        let response = agent_call(&serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "view",
            "params": { "path": path.display().to_string(), "around": "sum" }
        }));
        assert_eq!(
            response["error"]["code"],
            serde_json::json!(-32602),
            "an anchor with nothing to narrow is an invalid request: {response}"
        );
    });
}

/// Ask the agent for one function and return the text it read back.
fn agent_view(path: &Path, name: &str) -> String {
    let response = agent_call(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "view",
        "params": { "path": path.display().to_string(), "fn": name }
    }));
    response["result"]["view"]["text"]
        .as_str()
        .expect("the agent answers a view with its text")
        .to_string()
}

/// Send one framed request to a fresh agent session and read the response.
fn agent_call(request: &serde_json::Value) -> serde_json::Value {
    use std::process::{Command, Stdio};

    let body = serde_json::to_string(request).expect("the request serializes");
    let mut child = Command::new(assert_cmd::cargo_bin!("miri"))
        .arg("agent")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("the agent starts");

    {
        let stdin = child.stdin.as_mut().expect("the session accepts input");
        write!(stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .expect("the request is sent");
    }

    let output = child.wait_with_output().expect("the session ends");
    let text = String::from_utf8_lossy(&output.stdout).into_owned();
    let start = text.find('{').expect("the response carries a body");
    serde_json::from_str(&text[start..]).expect("the response body parses")
}

/// A fixture whose declarations sit at lines the assertions name outright.
///
/// Line 1 is the doc comment, so `stamp` is declared on line 2 and its body
/// ends on line 3; `Ledger` runs from line 5 to line 13.
const LOCATED: &str = "\
// Stamps a value.
fn stamp(v int) int
    return v + 1

class Ledger
    private fn audit() int
        return 0

    protected fn seal() int
        return 1

    fn total() int
        return 2
";

#[test]
fn test_outline_json_locates_each_declaration_in_the_source() {
    with_source(LOCATED, |path| {
        let (stdout, _, ok) = view(&[
            "--outline",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(ok, "an outline of a well-formed file should succeed");
        let view = envelope(&stdout).view.expect("an outline carries a view");
        let located: Vec<(String, Option<usize>, Option<usize>)> = view
            .spans
            .iter()
            .map(|span| {
                (
                    span.name.clone().unwrap_or_default(),
                    span.line,
                    span.end_line,
                )
            })
            .collect();
        assert_eq!(
            located,
            vec![
                ("stamp".to_string(), Some(2), Some(3)),
                ("Ledger".to_string(), Some(5), Some(13)),
                ("audit".to_string(), Some(6), Some(7)),
                ("seal".to_string(), Some(9), Some(10)),
                ("total".to_string(), Some(12), Some(13)),
            ],
            "every outline entry carries the source lines it was read from"
        );
    });
}

#[test]
fn test_fn_json_locates_the_declaration_in_the_source() {
    with_source(LOCATED, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "Ledger.total",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(ok, "viewing a method should succeed");
        let view = envelope(&stdout)
            .view
            .expect("a function read carries a view");
        let span = view.spans.first().expect("the declaration is recorded");
        assert_eq!(
            (span.line, span.end_line),
            (Some(12), Some(13)),
            "the method's own source lines travel with the rendering"
        );
    });
}

#[test]
fn test_around_json_locates_the_narrowed_block() {
    with_source(LOCATED, |path| {
        let (stdout, _, ok) = view(&[
            "--fn",
            "stamp",
            "--around",
            "return v + 1",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(ok, "narrowing to a block should succeed");
        let view = envelope(&stdout)
            .view
            .expect("a narrowed read carries a view");
        let span = view
            .spans
            .first()
            .expect("a narrowed block is recorded as a span of its own");
        assert_eq!(
            span.kind, "block",
            "the narrowed region is reported as a block"
        );
        assert_eq!(
            (span.line, span.end_line),
            (Some(3), Some(3)),
            "a narrowed block reports the source lines it covers"
        );
    });
}

#[test]
fn test_outline_public_hides_private_and_protected_members() {
    with_source(LOCATED, |path| {
        let (stdout, _, ok) = view(&[
            "--outline",
            "--public",
            path.to_str().expect("a utf-8 path"),
        ]);
        assert!(ok, "a public outline should succeed");
        assert!(stdout.contains("fn total()"), "a public method stays");
        assert!(!stdout.contains("audit"), "a private method is hidden");
        assert!(!stdout.contains("seal"), "a protected method is hidden");
    });
}

#[test]
fn test_outline_public_hides_runtime_bindings() {
    let source = "\
class Buffer
    runtime \"core\" fn miri_rt_buffer_new(size int) Self

    fn length() int
        return 0
";
    with_source(source, |path| {
        let target = path.to_str().expect("a utf-8 path");
        let (plain, _, _) = view(&["--outline", target]);
        assert!(
            plain.contains("runtime"),
            "the default outline still lists a runtime binding"
        );
        let (public, _, ok) = view(&["--outline", "--public", target]);
        assert!(ok, "a public outline should succeed");
        assert!(
            !public.contains("runtime"),
            "a public outline omits runtime bindings"
        );
        assert!(public.contains("fn length()"), "a public method stays");
    });
}

#[test]
fn test_neither_fn_nor_outline_names_both_alternatives() {
    with_source(LOCATED, |path| {
        let (_, stderr, ok) = view(&[path.to_str().expect("a utf-8 path")]);
        assert!(!ok, "a read with no shape should fail");
        assert!(
            stderr.contains("--fn <NAME>") && stderr.contains("--outline"),
            "the failure names both ways to ask for a shape, got: {stderr}"
        );
    });
}

#[test]
fn test_public_cannot_be_combined_with_fn() {
    with_source(LOCATED, |path| {
        let (_, stderr, ok) = view(&[
            "--fn",
            "stamp",
            "--public",
            path.to_str().expect("a utf-8 path"),
        ]);
        assert!(!ok, "--public applies to an outline only");
        assert!(
            stderr.contains("--public"),
            "the failure names the flag that does not apply, got: {stderr}"
        );
    });
}

#[test]
fn test_help_opens_with_one_sentence() {
    let output = miri_cmd()
        .args(["view", "--help"])
        .output()
        .expect("the help runs");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let first = stdout.lines().next().unwrap_or_default();
    assert_eq!(
        first, "Read part of a Miri source file: one function, or an outline of it",
        "the summary is one sentence, not two run together"
    );
    assert!(
        stdout.contains("<--fn <NAME>|--outline>"),
        "the usage line offers both shapes, got: {stdout}"
    );
}

/// Declaration shapes whose extent is easy to compute one line short.
///
/// An attribute sits above its function, a `:` body shares the signature's
/// line, a `match` ends inside a nested arm, a lambda carries a body of its
/// own, a generic struct ends on a field, and the last declaration runs to the
/// end of the file with no closing token and no trailing newline.
const SHAPES: &str = "@must_use
fn attributed(x int) int
    return x + 1

fn single_line(x int) int: x * 2

fn matched(v int) int
    match v
        1: 1
        _: 0

fn lambda_holder() int
    let f = fn (x int) int: x + 1
    return f(1)

struct Pair<A>
    left A
    right A

fn at_eof() int
    return 9";

#[test]
fn test_outline_locates_declarations_whose_extent_is_easy_to_cut_short() {
    with_source(SHAPES, |path| {
        let (stdout, _, ok) = view(&[
            "--outline",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(ok, "an outline of a well-formed file should succeed");
        let view = envelope(&stdout).view.expect("an outline carries a view");
        let located: Vec<(String, Option<usize>, Option<usize>)> = view
            .spans
            .iter()
            .map(|span| {
                (
                    span.name.clone().unwrap_or_default(),
                    span.line,
                    span.end_line,
                )
            })
            .collect();
        assert_eq!(
            located,
            vec![
                // The attribute on line 1 is not part of the declaration's own
                // line, the way a doc comment above it is not.
                ("attributed".to_string(), Some(2), Some(3)),
                ("single_line".to_string(), Some(5), Some(5)),
                ("matched".to_string(), Some(7), Some(10)),
                ("lambda_holder".to_string(), Some(12), Some(14)),
                ("Pair".to_string(), Some(16), Some(18)),
                ("at_eof".to_string(), Some(20), Some(21)),
            ],
            "each declaration reaches the last line it actually covers"
        );
    });
}

#[test]
fn test_located_spans_are_consistent_wherever_they_are_reported() {
    with_source(SHAPES, |path| {
        let (stdout, _, ok) = view(&[
            "--outline",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(ok, "an outline of a well-formed file should succeed");
        let view = envelope(&stdout).view.expect("an outline carries a view");
        let line_count = SHAPES.lines().count();
        for span in &view.spans {
            assert_eq!(
                span.line.is_some(),
                span.end_line.is_some(),
                "a span carries both source lines or neither"
            );
            let (Some(line), Some(end_line)) = (span.line, span.end_line) else {
                continue;
            };
            assert!(line >= 1, "source lines are 1-based, got {line}");
            assert!(
                end_line >= line,
                "a declaration does not end before it starts: {line}..{end_line}"
            );
            assert!(
                end_line <= line_count,
                "a declaration does not run past the end of the file: {end_line} > {line_count}"
            );
        }
    });
}

#[test]
fn test_public_outline_keeps_a_container_whose_every_member_is_hidden() {
    let source = "class Hidden
    private fn secret() int: 0

    runtime \"core\" fn miri_rt_hidden() int
";
    with_source(source, |path| {
        let (stdout, _, ok) = view(&[
            "--outline",
            "--public",
            path.to_str().expect("a utf-8 path"),
            "--format",
            "json",
        ]);
        assert!(
            ok,
            "a public outline of a fully hidden class still succeeds"
        );
        let view = envelope(&stdout).view.expect("an outline carries a view");
        assert_eq!(
            view.text, "class Hidden\n",
            "the container survives even when nothing inside it does"
        );
        assert_eq!(view.spans.len(), 1, "only the container is recorded");
    });
}
