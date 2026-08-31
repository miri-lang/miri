// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Tests for `miri agent`, the JSON-RPC session over stdin and stdout.
//!
//! Each test drives a real compiler process through the framing a client would
//! use, so what is exercised is the protocol as shipped rather than the handler
//! functions behind it.

use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::time::Instant;

use serde_json::{json, Value};

/// A live `miri agent` process a test can exchange messages with.
struct Session {
    process: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
}

impl Session {
    /// Start a session whose working directory is `directory`.
    fn start(directory: &Path) -> Self {
        let mut process = Command::new(assert_cmd::cargo_bin!("miri"))
            .arg("agent")
            .current_dir(directory)
            .env(
                "MIRI_STDLIB_PATH",
                PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib"),
            )
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("the compiler binary should start");

        let input = process.stdin.take().expect("stdin was piped");
        let output = BufReader::new(process.stdout.take().expect("stdout was piped"));
        Self {
            process,
            input,
            output,
        }
    }

    /// Send one framed message.
    fn send(&mut self, message: &Value) {
        let body = serde_json::to_string(message).expect("the message should serialize");
        write!(self.input, "Content-Length: {}\r\n\r\n{}", body.len(), body)
            .expect("the session should accept the message");
        self.input.flush().expect("the message should be sent");
    }

    /// Call `method` with `params` and return the response.
    fn call(&mut self, id: i64, method: &str, params: Value) -> Value {
        self.send(&json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }));
        self.receive()
    }

    /// Read one framed response.
    fn receive(&mut self) -> Value {
        let mut length = None;
        loop {
            let mut line = String::new();
            let read = self
                .output
                .read_line(&mut line)
                .expect("the session should still be writing");
            assert!(read > 0, "the session ended before answering");

            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                break;
            }
            if let Some(value) = line.strip_prefix("Content-Length:") {
                length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .expect("the length header should be a number"),
                );
            }
        }

        let length = length.expect("every frame declares its length");
        let mut body = vec![0u8; length];
        self.output
            .read_exact(&mut body)
            .expect("the frame should carry the bytes it declared");
        serde_json::from_slice(&body).expect("a response should be JSON")
    }

    /// Close the session and return what it wrote to stderr.
    fn finish(mut self) -> String {
        drop(self.input);
        let mut stderr = String::new();
        if let Some(mut handle) = self.process.stderr.take() {
            let _ = handle.read_to_string(&mut stderr);
        }
        let status = self.process.wait().expect("the session should end");
        assert!(
            status.success(),
            "closing stdin should end the session cleanly, got {:?}",
            status
        );
        stderr
    }
}

/// Write `contents` to `name` inside a fresh directory and return the directory.
fn project(name: &str, files: &[(&str, &str)]) -> tempfile::TempDir {
    let directory = tempfile::Builder::new()
        .prefix(&format!("miri-agent-{}-", name))
        .tempdir()
        .expect("a temporary directory should be available");
    for (file, contents) in files {
        std::fs::write(directory.path().join(file), contents).expect("the fixture should write");
    }
    directory
}

/// A program whose only fault is an assignment to an immutable binding.
const REASSIGNED_LET: &str =
    "fn main():\n    let total = 1\n    total = 2\n    println(\"{total}\")\n";

#[test]
fn test_the_handshake_reports_the_schema_version_the_cli_emits() {
    // The handshake is how a client learns whether it understands the envelopes
    // this session will send, so the number must be the one the command line
    // puts in an envelope.
    let directory = project("handshake", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "initialize", json!({}));
    let info = &response["result"]["serverInfo"];

    assert_eq!(response["id"], json!(1));
    assert_eq!(info["name"], json!("miri"));
    assert_eq!(
        info["schemaVersion"],
        json!(miri::diagnostics::json::SCHEMA_VERSION),
        "the handshake must name the schema the envelopes use"
    );
    assert_eq!(
        info["version"],
        json!(miri::cli::version_string()),
        "the handshake must name the compiler that is serving it"
    );
    session.finish();
}

#[test]
fn test_the_handshake_names_served_and_reserved_methods_apart() {
    let directory = project("capabilities", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "initialize", json!({}));
    let capabilities = &response["result"]["capabilities"];
    let served = capabilities["methods"]
        .as_array()
        .expect("the served methods are a list");
    let reserved = capabilities["reservedMethods"]
        .as_array()
        .expect("the reserved methods are a list");

    assert!(served.contains(&json!("check")));
    assert!(served.contains(&json!("fixApply")));
    assert!(served.contains(&json!("view")));
    assert!(served.contains(&json!("patch")));
    assert!(served.contains(&json!("skillsGet")));
    assert!(reserved.contains(&json!("tokens")));
    for method in served {
        assert!(
            !reserved.contains(method),
            "{} is offered as both served and reserved",
            method
        );
    }
    session.finish();
}

#[test]
fn test_one_session_checks_plans_applies_and_rechecks_clean() {
    // The acceptance criterion for the command: a client repairs a program
    // without leaving the session or restarting the compiler.
    let directory = project("repair-loop", &[("main.mi", REASSIGNED_LET)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let checked = session.call(1, "check", json!({ "path": path }));
    let envelope = &checked["result"];
    assert_eq!(
        envelope["ok"],
        json!(false),
        "the program assigns to an immutable binding, so the check must fail"
    );
    assert!(
        checked["error"].is_null(),
        "a program that does not compile is an answer, not a protocol failure: {}",
        checked
    );
    let reported = envelope["diagnostics"][0]["code"]
        .as_str()
        .expect("the diagnostic carries a code")
        .to_string();

    let planned = session.call(2, "fixPlan", json!({ "path": path }));
    let repair = &planned["result"]["diagnostics"][0]["repair"];
    assert!(
        !repair.is_null(),
        "the diagnostic {} should carry a repair: {}",
        reported,
        planned
    );
    assert_eq!(
        std::fs::read_to_string(&directory.path().join("main.mi")).expect("the file exists"),
        REASSIGNED_LET,
        "planning must not modify the file"
    );

    let applied = session.call(3, "fixApply", json!({ "path": path }));
    assert_eq!(
        applied["result"]["ok"],
        json!(true),
        "the repair is a local edit and should apply: {}",
        applied
    );
    let rewritten =
        std::fs::read_to_string(directory.path().join("main.mi")).expect("the file exists");
    assert!(
        rewritten.contains("var total = 1"),
        "the apply should have rewritten the declaration, found: {:?}",
        rewritten
    );

    let rechecked = session.call(4, "check", json!({ "path": path }));
    assert_eq!(
        rechecked["result"]["ok"],
        json!(true),
        "the repaired program should check clean: {}",
        rechecked
    );
    assert_eq!(
        rechecked["result"]["diagnostics"]
            .as_array()
            .expect("diagnostics is a list")
            .len(),
        0
    );
    session.finish();
}

#[test]
fn test_a_program_that_does_not_compile_is_a_result_not_a_protocol_error() {
    // A compile error is the compiler answering, exactly as `miri check` prints
    // an envelope and exits 1. Reporting it as a JSON-RPC error would tell a
    // client the request failed when it succeeded.
    let directory = project(
        "broken",
        &[("main.mi", "fn main():\n    undefined_name()\n")],
    );
    let path = directory.path().join("main.mi");
    let mut session = Session::start(directory.path());

    let response = session.call(1, "check", json!({ "path": path.to_str().unwrap() }));

    assert!(
        response["error"].is_null(),
        "a compile error must not be reported as a protocol error: {}",
        response
    );
    assert_eq!(response["result"]["ok"], json!(false));
    assert!(
        !response["result"]["diagnostics"]
            .as_array()
            .expect("diagnostics is a list")
            .is_empty(),
        "the failure should be described by diagnostics"
    );
    session.finish();
}

#[test]
fn test_explain_answers_over_the_session() {
    let directory = project("explain", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "explain", json!({ "code": "MER_TYP_030" }));
    let explanation = &response["result"]["explanation"];

    assert_eq!(response["result"]["ok"], json!(true));
    assert_eq!(explanation["code"], json!("MER_TYP_030"));
    assert!(
        !explanation["rule"]
            .as_str()
            .expect("the rule is text")
            .is_empty(),
        "an explanation should have a body"
    );
    session.finish();
}

#[test]
fn test_an_unknown_code_is_answered_as_a_diagnostic() {
    let directory = project("explain-unknown", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "explain", json!({ "code": "MER_NOPE_999" }));

    assert!(response["error"].is_null());
    assert_eq!(response["result"]["ok"], json!(false));
    assert_eq!(
        response["result"]["diagnostics"][0]["code"],
        json!("MER_BLD_001")
    );
    session.finish();
}

#[test]
fn test_skills_get_answers_with_the_same_text_the_command_line_writes() {
    let directory = project("skills", &[]);
    let mut session = Session::start(directory.path());

    let one = session.call(1, "skillsGet", json!({ "name": "miri-lang" }));
    let skills = one["result"]["skills"]
        .as_array()
        .expect("the answer carries the skill");
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0]["name"], json!("miri-lang"));

    // A tool driving the compiler must read the same skill a person does.
    let printed = crate::utils::miri_cmd()
        .args(["skill", "show", "miri-lang"])
        .output()
        .expect("the skill command runs");
    let shown = String::from_utf8_lossy(&printed.stdout).into_owned();
    assert_eq!(skills[0]["body"], json!(crate::cli::skill::body_of(&shown)));

    let all = session.call(2, "skillsGet", json!({}));
    assert_eq!(
        all["result"]["skills"]
            .as_array()
            .expect("the whole catalogue comes back")
            .len(),
        skills.len() + 2
    );
    session.finish();
}

#[test]
fn test_skills_get_refuses_a_name_that_is_not_a_string() {
    let directory = project("skills-typed", &[]);
    let mut session = Session::start(directory.path());

    // Reading a wrong-typed name as an absent one would hand back the whole
    // catalogue to a caller that asked for one skill, and call it success.
    for wrong in [
        json!(123),
        json!(["miri-lang"]),
        json!({ "name": "miri-lang" }),
    ] {
        let answer = session.call(1, "skillsGet", json!({ "name": wrong }));
        assert_eq!(answer["error"]["code"], json!(-32602));
    }

    let null_name = session.call(2, "skillsGet", json!({ "name": null }));
    assert_eq!(
        null_name["result"]["skills"]
            .as_array()
            .expect("a null name reads as no name")
            .len(),
        3
    );
    session.finish();
}

#[test]
fn test_skills_get_refuses_a_name_it_does_not_carry() {
    let directory = project("skills-unknown", &[]);
    let mut session = Session::start(directory.path());

    let answer = session.call(1, "skillsGet", json!({ "name": "not-a-skill" }));
    assert_eq!(answer["result"]["ok"], json!(false));
    assert_eq!(
        answer["result"]["diagnostics"][0]["code"],
        json!("MER_BLD_013")
    );
    session.finish();
}

#[test]
fn test_a_reserved_method_is_distinguishable_from_a_misspelled_one() {
    // A client should be able to tell "this is coming in a later release" from
    // "you sent a name that will never exist", and say so to its user.
    let directory = project("reserved", &[]);
    let mut session = Session::start(directory.path());

    let reserved = session.call(1, "tokens", json!({}));
    let unknown = session.call(2, "chekc", json!({}));

    assert_eq!(reserved["error"]["code"], json!(-32601));
    assert_eq!(reserved["error"]["data"]["reserved"], json!(true));
    assert_eq!(reserved["error"]["data"]["method"], json!("tokens"));

    assert_eq!(unknown["error"]["code"], json!(-32601));
    assert_eq!(unknown["error"]["data"]["reserved"], json!(false));
    session.finish();
}

#[test]
fn test_a_request_naming_no_file_is_refused_without_ending_the_session() {
    let directory = project("no-path", &[]);
    let mut session = Session::start(directory.path());

    let refused = session.call(1, "check", json!({}));
    assert_eq!(refused["error"]["code"], json!(-32602));

    // The session must survive a bad request; a client should not have to
    // restart the compiler because it sent one.
    let after = session.call(2, "initialize", json!({}));
    assert_eq!(after["result"]["serverInfo"]["name"], json!("miri"));
    session.finish();
}

#[test]
fn test_a_file_that_cannot_be_read_is_a_request_failure() {
    let directory = project("missing-file", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "check", json!({ "path": "no-such-file.mi" }));

    assert_eq!(
        response["error"]["code"],
        json!(-32602),
        "a file the compiler was never given is a fault in the request, not a verdict on a program"
    );
    session.finish();
}

#[test]
fn test_a_malformed_message_is_answered_and_the_session_continues() {
    let directory = project("malformed", &[]);
    let mut session = Session::start(directory.path());

    let body = r#"{"jsonrpc":"2.0","id":1,"method":}"#;
    write!(
        session.input,
        "Content-Length: {}\r\n\r\n{}",
        body.len(),
        body
    )
    .expect("the session should accept the bytes");
    session.input.flush().expect("the bytes should be sent");

    let response = session.receive();
    assert_eq!(response["error"]["code"], json!(-32700));

    let after = session.call(2, "initialize", json!({}));
    assert_eq!(after["id"], json!(2));
    session.finish();
}

#[test]
fn test_a_notification_is_not_answered() {
    let directory = project("notification", &[]);
    let mut session = Session::start(directory.path());

    // A message with no identifier expects no response. Sending one followed by
    // a request proves the request's answer is not the notification's.
    session.send(&json!({ "jsonrpc": "2.0", "method": "initialize" }));
    let response = session.call(9, "initialize", json!({}));

    assert_eq!(
        response["id"],
        json!(9),
        "the only answer should belong to the request that asked for one"
    );
    session.finish();
}

#[test]
fn test_a_request_withdrawn_before_it_starts_is_reported_cancelled() {
    // Both messages are written before the session reads either, so the
    // cancellation is recorded before the request it names is taken up.
    let directory = project(
        "cancel",
        &[("main.mi", "fn main():\n    println(\"hi\")\n")],
    );
    let path = directory.path().join("main.mi");
    let mut session = Session::start(directory.path());

    session.send(&json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": 1 },
    }));
    session.send(&json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "check",
        "params": { "path": path.to_str().unwrap() },
    }));

    let response = session.receive();
    assert_eq!(response["id"], json!(1));
    assert_eq!(
        response["error"]["code"],
        json!(-32800),
        "a withdrawn request should be reported cancelled: {}",
        response
    );
    session.finish();
}

#[test]
fn test_a_cancellation_does_not_withdraw_a_later_reuse_of_the_identifier() {
    // A client may reuse an identifier once the request carrying it is done.
    // A cancellation left lying around would withdraw the reuse.
    let directory = project(
        "cancel-reuse",
        &[("main.mi", "fn main():\n    println(\"hi\")\n")],
    );
    let path = directory.path().join("main.mi");
    let path = path.to_str().unwrap();
    let mut session = Session::start(directory.path());

    session.send(&json!({
        "jsonrpc": "2.0",
        "method": "$/cancelRequest",
        "params": { "id": 5 },
    }));
    session.send(&json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "check",
        "params": { "path": path },
    }));
    let withdrawn = session.receive();
    assert_eq!(withdrawn["error"]["code"], json!(-32800));

    let reused = session.call(5, "check", json!({ "path": path }));
    assert!(
        reused["error"].is_null(),
        "the identifier was free to reuse: {}",
        reused
    );
    assert_eq!(reused["result"]["ok"], json!(true));
    session.finish();
}

#[test]
fn test_nothing_but_the_protocol_is_written_to_stdout() {
    // A stray line on stdout would sit inside a frame and desynchronise the
    // stream. Reading exactly as many frames as there were requests, and then
    // finding the stream at its end, is what proves nothing else was written.
    let directory = project("stdout", &[("main.mi", REASSIGNED_LET)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().unwrap();
    let mut session = Session::start(directory.path());

    session.call(1, "initialize", json!({}));
    session.call(2, "check", json!({ "path": path }));
    session.call(3, "fixApply", json!({ "path": path }));

    drop(session.input);
    let mut rest = Vec::new();
    session
        .output
        .read_to_end(&mut rest)
        .expect("the stream should end");
    assert!(
        rest.is_empty(),
        "stdout carried something that was not a response frame: {:?}",
        String::from_utf8_lossy(&rest)
    );
    assert!(session
        .process
        .wait()
        .expect("the session should end")
        .success());
}

#[test]
fn test_an_incremental_check_in_a_hundred_module_project_answers_within_the_budget() {
    // The contract a tool depends on: once the session is warm, one file's
    // check comes back fast enough to sit inside an edit loop.
    //
    // The measurement is the fastest of several warm round-trips rather than
    // the slowest. What is being asked is whether the compiler *can* answer
    // within the budget, and this suite runs its tests in parallel: a slowest
    // sample measures how busy the machine was, not how fast the check is. The
    // binary under test is also the unoptimised one the harness builds, which
    // costs roughly five times the shipped compiler. Both effects inflate the
    // worst sample and neither says anything about the product.
    //
    // The budget is still the real one. On an idle machine this build answers
    // in about 125ms and the release build in about 25ms, so a regression that
    // cost the check four times its current work would fail this.
    const BUDGET_MS: u128 = 500;
    const MODULES: usize = 100;
    const SAMPLES: usize = 10;

    let mut files: Vec<(String, String)> = (0..MODULES)
        .map(|index| {
            (
                format!("mod{:03}.mi", index),
                format!(
                    "public fn helper{index:03}(value int) int:\n    return value + {index}\n",
                    index = index
                ),
            )
        })
        .collect();
    let imports: String = (0..MODULES)
        .map(|index| format!("use local.mod{:03}\n", index))
        .collect();
    files.push((
        "main.mi".to_string(),
        format!(
            "{}\nfn main():\n    println(\"{{helper000(1)}}\")\n",
            imports
        ),
    ));

    let borrowed: Vec<(&str, &str)> = files
        .iter()
        .map(|(name, body)| (name.as_str(), body.as_str()))
        .collect();
    let directory = project("latency", &borrowed);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");

    let mut session = Session::start(directory.path());

    // The first check pays for loading the standard library; the budget is for
    // a session that has already done that.
    let warmup = session.call(0, "check", json!({ "path": path }));
    assert_eq!(
        warmup["result"]["ok"],
        json!(true),
        "the generated project should check clean: {}",
        warmup
    );

    let mut fastest = u128::MAX;
    for attempt in 1..=SAMPLES {
        let started = Instant::now();
        let response = session.call(attempt as i64, "check", json!({ "path": path }));
        let elapsed = started.elapsed().as_millis();
        assert_eq!(
            response["result"]["ok"],
            json!(true),
            "every sampled check should still pass: {}",
            response
        );
        fastest = fastest.min(elapsed);
    }

    assert!(
        fastest <= BUDGET_MS,
        "the fastest of {} warm checks of a {}-module project took {}ms, over the {}ms budget",
        SAMPLES,
        MODULES,
        fastest,
        BUDGET_MS
    );
    session.finish();
}

/// A program whose only fault is an assignment to a publicly visible
/// module-scope binding. Rewriting it to `var` widens a surface other modules
/// can observe, so the repair is classified `api-changing`.
const REASSIGNED_PUBLIC_LET: &str =
    "public let total = 1\n\nfn main():\n    total = 2\n    println(\"{total}\")\n";

#[test]
fn test_a_risky_repair_is_refused_unless_the_caller_allows_it() {
    // There is no terminal to confirm at over a session, so the caller says
    // outright whether a repair the compiler classes as risky may be written.
    // The default must be that it may not.
    let directory = project("refuse-risky", &[("main.mi", REASSIGNED_PUBLIC_LET)]);
    let file = directory.path().join("main.mi");
    let path = file.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let refused = session.call(1, "fixApply", json!({ "path": path }));

    assert_eq!(
        refused["result"]["ok"],
        json!(false),
        "an api-changing repair must not be applied by default: {}",
        refused
    );
    let codes: Vec<&str> = refused["result"]["diagnostics"]
        .as_array()
        .expect("diagnostics is a list")
        .iter()
        .filter_map(|entry| entry["code"].as_str())
        .collect();
    assert!(
        codes.contains(&"MER_BLD_002"),
        "the refusal should carry the code that names it, saw {:?}",
        codes
    );
    assert_eq!(
        std::fs::read_to_string(&file).expect("the file exists"),
        REASSIGNED_PUBLIC_LET,
        "a refused apply must leave the file exactly as it was"
    );
    session.finish();
}

#[test]
fn test_a_risky_repair_is_applied_when_the_caller_allows_it() {
    let directory = project("allow-risky", &[("main.mi", REASSIGNED_PUBLIC_LET)]);
    let file = directory.path().join("main.mi");
    let path = file.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let applied = session.call(1, "fixApply", json!({ "path": path, "allowRisky": true }));

    assert_eq!(
        applied["result"]["ok"],
        json!(true),
        "the caller allowed the risk, so the repair should be written: {}",
        applied
    );
    assert!(
        std::fs::read_to_string(&file)
            .expect("the file exists")
            .contains("public var total = 1"),
        "the declaration should now be mutable"
    );
    session.finish();
}

#[test]
fn test_a_check_reporting_only_warnings_still_reports_ok() {
    // Warnings never fail a check. A client that treated them as failure would
    // refuse to proceed on a program the compiler accepts.
    let source = "@deprecated(\"use current\")\nfn old() int:\n    return 1\n\nfn current() int:\n    return 2\n\nfn main():\n    let value = old()\n    println(\"{value}\")\n";
    let directory = project("warnings-only", &[("main.mi", source)]);
    let path = directory.path().join("main.mi");
    let mut session = Session::start(directory.path());

    let response = session.call(1, "check", json!({ "path": path.to_str().unwrap() }));
    let diagnostics = response["result"]["diagnostics"]
        .as_array()
        .expect("diagnostics is a list");

    assert_eq!(
        response["result"]["ok"],
        json!(true),
        "warnings do not fail a check: {}",
        response
    );
    assert_eq!(diagnostics.len(), 1, "the warning should still be reported");
    assert_eq!(diagnostics[0]["severity"], json!("warning"));
    session.finish();
}

#[test]
fn test_a_check_runs_with_mir_verification_when_asked() {
    let directory = project(
        "verify-mir",
        &[("main.mi", "fn main():\n    println(\"hi\")\n")],
    );
    let path = directory.path().join("main.mi");
    let mut session = Session::start(directory.path());

    let response = session.call(
        1,
        "check",
        json!({ "path": path.to_str().unwrap(), "verifyMir": true }),
    );

    assert!(response["error"].is_null(), "{}", response);
    assert_eq!(response["result"]["ok"], json!(true));
    session.finish();
}

#[test]
fn test_planning_a_file_with_nothing_to_repair_reports_no_repairs() {
    let directory = project(
        "nothing-to-repair",
        &[("main.mi", "fn main():\n    println(\"hi\")\n")],
    );
    let path = directory.path().join("main.mi");
    let path = path.to_str().unwrap();
    let mut session = Session::start(directory.path());

    let planned = session.call(1, "fixPlan", json!({ "path": path }));
    assert_eq!(planned["result"]["ok"], json!(true));
    assert!(planned["result"]["diagnostics"]
        .as_array()
        .expect("a list")
        .is_empty());

    // Applying nothing is a success, not a failure.
    let applied = session.call(2, "fixApply", json!({ "path": path }));
    assert_eq!(applied["result"]["ok"], json!(true), "{}", applied);
    session.finish();
}

#[test]
fn test_applying_twice_leaves_the_file_repaired_once() {
    // The second call finds nothing left to repair. It must not fail, and must
    // not edit the file again.
    let directory = project("apply-twice", &[("main.mi", REASSIGNED_LET)]);
    let file = directory.path().join("main.mi");
    let path = file.to_str().unwrap();
    let mut session = Session::start(directory.path());

    let first = session.call(1, "fixApply", json!({ "path": path }));
    assert_eq!(first["result"]["ok"], json!(true), "{}", first);
    let after_first = std::fs::read_to_string(&file).expect("the file exists");

    let second = session.call(2, "fixApply", json!({ "path": path }));
    assert_eq!(second["result"]["ok"], json!(true), "{}", second);

    assert_eq!(
        std::fs::read_to_string(&file).expect("the file exists"),
        after_first,
        "a second apply with nothing to do must leave the file alone"
    );
    session.finish();
}

#[test]
fn test_every_file_taking_method_refuses_a_request_naming_no_file() {
    let directory = project("no-path-any-method", &[]);
    let mut session = Session::start(directory.path());

    for (id, method) in [(1, "check"), (2, "fixPlan"), (3, "fixApply")] {
        let refused = session.call(id, method, json!({}));
        assert_eq!(
            refused["error"]["code"],
            json!(-32602),
            "{} must refuse a request naming no file: {}",
            method,
            refused
        );
    }
    session.finish();
}

#[test]
fn test_explain_refuses_a_request_naming_no_code() {
    let directory = project("no-code", &[]);
    let mut session = Session::start(directory.path());

    let refused = session.call(1, "explain", json!({}));

    assert_eq!(refused["error"]["code"], json!(-32602), "{}", refused);
    session.finish();
}

/// A program with one function whose body can be anchored on.
const PATCHABLE: &str = "fn total(a int, b int) int
    return a + b
";

#[test]
fn test_a_session_reads_a_function_then_edits_it() {
    // The two surfaces are meant to compose: what `view` renders is the text
    // `patch` anchors against, and the edit answers with the diagnostics of the
    // program it produced rather than with a bare acknowledgement.
    let directory = project("patch-loop", &[("main.mi", PATCHABLE)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let read = session.call(1, "view", json!({ "path": path, "fn": "total" }));
    let rendered = read["result"]["view"]["text"]
        .as_str()
        .expect("the view carries its text");
    assert!(
        rendered.contains("return a + b"),
        "the rendering holds the line to anchor on: {rendered}"
    );

    let edited = session.call(
        2,
        "patch",
        json!({
            "path": path,
            "operations": [{ "function": "total", "old": "a + b", "new": "a * b" }],
        }),
    );
    assert!(
        edited["error"].is_null(),
        "an edit is an answer, not a protocol failure: {edited}"
    );
    let envelope = &edited["result"];
    assert_eq!(envelope["ok"], json!(true), "the edit checks: {edited}");
    assert_eq!(envelope["command"], json!("patch"));
    assert_eq!(envelope["patch"]["revalidations"], json!(1));
    assert_eq!(envelope["patch"]["fileWritten"], json!(true));

    let written = std::fs::read_to_string(path).expect("the edited file can be read");
    assert!(written.contains("return a * b"), "got: {written}");

    session.finish();
}

#[test]
fn test_a_session_reports_an_edit_that_does_not_check() {
    let directory = project("patch-rejected", &[("main.mi", PATCHABLE)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let edited = session.call(
        2,
        "patch",
        json!({
            "path": path,
            "operations": [{ "function": "total", "old": "a + b", "new": "\"text\"" }],
        }),
    );
    let envelope = &edited["result"];
    assert_eq!(
        envelope["ok"],
        json!(false),
        "an edit producing a type error does not succeed: {edited}"
    );
    assert_eq!(
        std::fs::read_to_string(path).expect("the file can be read"),
        PATCHABLE,
        "a rejected edit leaves the file as it was"
    );

    session.finish();
}

#[test]
fn test_a_session_can_insert_a_declaration() {
    let source = "fn helper() int
    return 42

fn main()
    println(\"ok\")
";
    let directory = project("patch-insert", &[("main.mi", source)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let edited = session.call(
        2,
        "patch",
        json!({
            "path": path,
            "operations": [{ "function": "answer", "insert": "fn answer() int\n    return 43" }],
        }),
    );
    assert!(
        edited["error"].is_null(),
        "an insert is an answer, not a protocol failure: {edited}"
    );
    let envelope = &edited["result"];
    assert_eq!(envelope["ok"], json!(true), "the insert checks: {edited}");
    assert_eq!(envelope["command"], json!("patch"));
    assert_eq!(envelope["patch"]["revalidations"], json!(1));
    assert_eq!(envelope["patch"]["fileWritten"], json!(true));

    let written = std::fs::read_to_string(path).expect("the edited file can be read");
    assert!(
        written.contains("fn answer()"),
        "inserted declaration should be in file: {written}"
    );

    session.finish();
}

#[test]
fn test_patch_with_unknown_mode_is_rejected() {
    let directory = project("patch-unknown-mode", &[("main.mi", PATCHABLE)]);
    let path = directory.path().join("main.mi");
    let path = path.to_str().expect("the path is text");
    let mut session = Session::start(directory.path());

    let refused = session.call(
        1,
        "patch",
        json!({
            "path": path,
            "operations": [],
            "mode": "invalid"
        }),
    );

    assert_eq!(
        refused["error"]["code"],
        json!(-32602),
        "patch with unknown mode must be rejected with -32602"
    );

    let error_msg = refused["error"]["message"]
        .as_str()
        .expect("error must have a message");

    // Verify the error message names the three valid modes
    assert!(
        error_msg.contains("apply"),
        "error should mention 'apply' mode: {}",
        error_msg
    );
    assert!(
        error_msg.contains("checkOnly"),
        "error should mention 'checkOnly' mode: {}",
        error_msg
    );
    assert!(
        error_msg.contains("dryRun"),
        "error should mention 'dryRun' mode: {}",
        error_msg
    );

    session.finish();
}

#[test]
fn test_schema_captures_every_parameter_used_by_handlers() {
    // This test ensures the schema is complete: every parameter name that
    // any handler reads is listed in that method's schema. The schema must
    // stay in sync as handlers evolve.
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    let source = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/cli/agent/mod.rs"),
    )
    .expect("agent/mod.rs should exist and be readable");

    // Extract parameters and build schema map
    let extracted = extract_parameter_reads(&source);

    let mut schema_params: HashMap<&str, HashSet<&str>> = HashMap::new();
    for method in miri::cli::agent::schema::METHODS {
        schema_params.insert(
            method.method,
            method.params.iter().map(|p| p.name).collect(),
        );
    }

    // Guard: total extracted ≥ 10
    let total: usize = extracted.values().map(|p| p.len()).sum();
    assert!(
        total >= 10,
        "should extract at least 10 parameter reads, but got {}",
        total
    );

    // Guard: key methods each have ≥ 1 parameter
    for method in &["check", "view", "patch", "fixApply", "explain", "skillsGet"] {
        assert!(
            !extracted[method].is_empty(),
            "method {} has no extracted parameters",
            method
        );
    }

    // Verify: extracted ⊆ schema
    for (method, params) in &extracted {
        for param in params {
            assert!(
                schema_params[method].contains(param),
                "method {} reads parameter {} but schema does not list it",
                method,
                param
            );
        }
    }

    // Verify: schema ⊆ extracted (no over-claiming)
    for (method, schema_set) in &schema_params {
        for schema_param in schema_set {
            assert!(
                extracted[method].contains(schema_param),
                "method {} has {} in schema but handler does not read it",
                method,
                schema_param
            );
        }
    }
}

/// Find the enclosing function name by searching backward through the source
/// for the most recent line matching "fn " at column 0 before the given chunk position.
///
/// This function takes a byte offset within a subslice and is only valid when
/// both source and chunk are from the same allocation (source contains chunk).
fn find_enclosing_fn<'a>(source: &'a str, chunk: &str) -> Option<&'a str> {
    let chunk_start = (chunk.as_ptr() as usize) - (source.as_ptr() as usize);
    let before_chunk = &source[..chunk_start];
    let mut best_fn: Option<&str> = None;

    // Find all function definitions before this chunk
    for line in before_chunk.lines().rev() {
        if line.starts_with("fn ") {
            if let Some(paren_pos) = line.find('(') {
                let fn_name = &line[3..paren_pos];
                best_fn = Some(fn_name);
                break;
            }
        }
    }

    best_fn
}

/// Extract all request parameters read by handlers in the given source.
///
/// Scans for three read idioms: `string_param(request, "X")`, `params.get("X")`,
/// and filters for actual request parameters (not metadata like "id" in $/cancelRequest).
/// Returns a map from method name to the set of parameter names extracted.
///
/// The scanned idiom list is intended as the **complete** set of request-parameter
/// read idioms in the module. We deliberately exclude `text("X")` because those are
/// field names within operation entries (in patch_operation), not request parameters;
/// the "operations" parameter itself is already captured by `params.get("operations")`.
fn extract_parameter_reads(
    source: &str,
) -> std::collections::HashMap<&str, std::collections::HashSet<&str>> {
    use std::collections::{HashMap, HashSet};

    // Map function names to served method names. This describes code structure.
    let mut fn_to_methods: HashMap<&str, Vec<&str>> = HashMap::new();
    fn_to_methods.insert("run_check", vec!["check"]);
    fn_to_methods.insert("run_explain", vec!["explain"]);
    fn_to_methods.insert("run_view", vec!["view"]);
    fn_to_methods.insert("run_patch", vec!["patch"]);
    fn_to_methods.insert("run_skills_get", vec!["skillsGet"]);
    // run_fix is called for both fixPlan and fixApply, but allowRisky is only read
    // when apply=true (fixApply). Use required_path to cover fixPlan's path read,
    // and map run_fix only to fixApply for its allowRisky read.
    fn_to_methods.insert("run_fix", vec!["fixApply"]);
    // patch_operations reads the "operations" request parameter
    fn_to_methods.insert("patch_operations", vec!["patch"]);
    // required_path is used by multiple file-taking methods
    fn_to_methods.insert(
        "required_path",
        vec!["check", "fixPlan", "fixApply", "view", "patch"],
    );

    // Map of schema methods for initialization
    let schema_methods: Vec<&str> = miri::cli::agent::schema::METHODS
        .iter()
        .map(|m| m.method)
        .collect();

    let mut extracted: HashMap<&str, HashSet<&str>> = HashMap::new();
    for method_name in schema_methods {
        extracted.insert(method_name, HashSet::new());
    }

    // Scan for string_param(request, "X")
    for chunk in source.split("string_param(request, \"") {
        if let Some(quote_pos) = chunk.find('"') {
            let param_name = &chunk[..quote_pos];
            if let Some(enclosing_fn) = find_enclosing_fn(source, chunk) {
                if let Some(methods) = fn_to_methods.get(enclosing_fn) {
                    for method in methods {
                        extracted.get_mut(method).unwrap().insert(param_name);
                    }
                } else if enclosing_fn != "initialize" && enclosing_fn != "serialize" {
                    // Panic on reads in unattributable handlers. "initialize" and "serialize"
                    // are explicitly allowed because they do not carry request parameters.
                    panic!(
                        "found parameter read {} in unattributable function {}",
                        param_name, enclosing_fn
                    );
                }
            }
        }
    }

    // Scan for params.get("X")
    for chunk in source.split("params.get(\"") {
        if let Some(quote_pos) = chunk.find('"') {
            let param_name = &chunk[..quote_pos];
            // Skip "id" which is from $/cancelRequest, not a method parameter
            if param_name == "id" {
                continue;
            }
            if let Some(enclosing_fn) = find_enclosing_fn(source, chunk) {
                if let Some(methods) = fn_to_methods.get(enclosing_fn) {
                    for method in methods {
                        extracted.get_mut(method).unwrap().insert(param_name);
                    }
                } else if enclosing_fn != "initialize" && enclosing_fn != "serialize" {
                    // Panic on reads in unattributable handlers. "initialize" and "serialize"
                    // are explicitly allowed because they do not carry request parameters.
                    panic!(
                        "found parameter read {} in unattributable function {}",
                        param_name, enclosing_fn
                    );
                }
            }
        }
    }

    extracted
}

#[test]
fn test_initialize_response_includes_method_schemas() {
    let directory = project("schema-handshake", &[]);
    let mut session = Session::start(directory.path());

    let response = session.call(1, "initialize", json!({}));
    let capabilities = &response["result"]["capabilities"];

    // The response should include methodSchemas alongside methods and reservedMethods
    assert!(
        !capabilities["methodSchemas"].is_null(),
        "capabilities should include methodSchemas"
    );

    let schemas = capabilities["methodSchemas"]
        .as_object()
        .expect("methodSchemas should be a map");

    // Verify methods is still an array of strings (non-breaking guarantee)
    let methods = capabilities["methods"]
        .as_array()
        .expect("methods should be an array of strings");
    for method in methods {
        assert!(
            method.is_string(),
            "every method name must be a string, not: {:?}",
            method
        );
    }

    // Check that there's a schema for each served method
    for method in methods {
        let method_name = method.as_str().unwrap();
        assert!(
            schemas.contains_key(method_name),
            "schema missing for method {}",
            method_name
        );

        let schema = &schemas[method_name];
        assert_eq!(
            schema["type"],
            json!("object"),
            "schema type should be object for {}",
            method_name
        );

        // Each schema must have $schema and properties
        assert!(
            schema.get("$schema").is_some(),
            "schema for {} must have $schema field",
            method_name
        );
        assert!(
            schema.get("properties").is_some(),
            "schema for {} must have properties field",
            method_name
        );
    }

    // Specific schema checks
    // view's around parameter must depend on fn (dependentRequired)
    let view_schema = &schemas["view"];
    assert_eq!(
        view_schema["dependentRequired"]["around"],
        json!(["fn"]),
        "view schema: around must require fn via dependentRequired"
    );

    // patch's mode parameter must have the three-value enum
    let patch_schema = &schemas["patch"];
    let mode_enum = &patch_schema["properties"]["mode"]["enum"];
    assert_eq!(
        mode_enum,
        &json!(["apply", "checkOnly", "dryRun"]),
        "patch schema: mode must have enum with apply, checkOnly, dryRun"
    );

    // initialize's schema must have empty properties (no parameters)
    let initialize_schema = &schemas["initialize"];
    let init_props = initialize_schema["properties"]
        .as_object()
        .expect("properties must be an object");
    assert!(
        init_props.is_empty(),
        "initialize schema: properties must be empty, but got: {:?}",
        init_props
    );

    session.finish();
}

#[test]
fn test_schema_does_not_over_claim() {
    // This test verifies the schema is not under-specified: every parameter marked
    // required in the schema is actually required by the handler, and omitting it
    // causes a -32602 error.
    let directory = project(
        "schema-over-claim",
        &[("main.mi", "fn main():\n    println(\"ok\")\n")],
    );
    let path = directory.path().join("main.mi");
    let path_str = path.to_str().unwrap();
    let mut session = Session::start(directory.path());

    // Test check: path is required, verifyMir is optional
    let with_required = session.call(1, "check", json!({ "path": path_str }));
    assert_eq!(
        with_required["error"]["code"],
        json!(null),
        "check with required path should not fail with -32602"
    );
    assert!(
        with_required["result"].is_object(),
        "check with required path should return a result"
    );

    let without_required = session.call(2, "check", json!({}));
    assert_eq!(
        without_required["error"]["code"],
        json!(-32602),
        "check without required path should fail with -32602"
    );

    // Test explain: code is required
    let with_code = session.call(3, "explain", json!({ "code": "MER_TYP_030" }));
    assert_eq!(
        with_code["error"]["code"],
        json!(null),
        "explain with required code should not fail with -32602"
    );

    let without_code = session.call(4, "explain", json!({}));
    assert_eq!(
        without_code["error"]["code"],
        json!(-32602),
        "explain without required code should fail with -32602"
    );

    // Test view: path is required, fn and around are optional
    let view_with_required = session.call(5, "view", json!({ "path": path_str }));
    assert_eq!(
        view_with_required["error"]["code"],
        json!(null),
        "view with required path should not fail with -32602"
    );

    let view_without_path = session.call(6, "view", json!({}));
    assert_eq!(
        view_without_path["error"]["code"],
        json!(-32602),
        "view without required path should fail with -32602"
    );

    // Test patch: path and operations are required
    let patch_with_required = session.call(
        7,
        "patch",
        json!({
            "path": path_str,
            "operations": []
        }),
    );
    assert_eq!(
        patch_with_required["error"]["code"],
        json!(null),
        "patch with required path and operations should not fail with -32602"
    );

    let patch_without_path = session.call(8, "patch", json!({ "operations": [] }));
    assert_eq!(
        patch_without_path["error"]["code"],
        json!(-32602),
        "patch without required path should fail with -32602"
    );

    let patch_without_operations = session.call(9, "patch", json!({ "path": path_str }));
    assert_eq!(
        patch_without_operations["error"]["code"],
        json!(-32602),
        "patch without required operations should fail with -32602"
    );

    // Test fixPlan: path is required
    let fix_plan_with_required = session.call(10, "fixPlan", json!({ "path": path_str }));
    assert_eq!(
        fix_plan_with_required["error"]["code"],
        json!(null),
        "fixPlan with required path should not fail with -32602"
    );

    let fix_plan_without_path = session.call(11, "fixPlan", json!({}));
    assert_eq!(
        fix_plan_without_path["error"]["code"],
        json!(-32602),
        "fixPlan without required path should fail with -32602"
    );

    // Test fixApply: path is required
    let fix_apply_with_required = session.call(12, "fixApply", json!({ "path": path_str }));
    assert_eq!(
        fix_apply_with_required["error"]["code"],
        json!(null),
        "fixApply with required path should not fail with -32602"
    );

    let fix_apply_without_path = session.call(13, "fixApply", json!({}));
    assert_eq!(
        fix_apply_without_path["error"]["code"],
        json!(-32602),
        "fixApply without required path should fail with -32602"
    );

    // Test skillsGet: name is optional
    let skills_without_optional = session.call(14, "skillsGet", json!({}));
    assert_eq!(
        skills_without_optional["error"]["code"],
        json!(null),
        "skillsGet without optional name should not fail with -32602"
    );

    // Test initialize: no parameters required
    let init = session.call(15, "initialize", json!({}));
    assert_eq!(
        init["error"]["code"],
        json!(null),
        "initialize with no parameters should not fail with -32602"
    );

    session.finish();
}

#[test]
fn test_check_with_missing_path_names_parameters_in_error() {
    let directory = project("schema-error", &[]);
    let mut session = Session::start(directory.path());

    let refused = session.call(1, "check", json!({}));
    let error_msg = refused["error"]["message"]
        .as_str()
        .expect("error should have a message");

    // The error message should include the full parameter list for this method
    let expected_params =
        miri::cli::agent::schema::accepted("check").expect("check should have a schema");
    assert!(
        error_msg.contains(&expected_params),
        "error should list all accepted parameters: expected '{}' in: {}",
        expected_params,
        error_msg
    );

    session.finish();
}

#[test]
fn test_view_with_around_but_no_fn_names_parameters_in_error() {
    let directory = project(
        "schema-view-error",
        &[("dummy.mi", "fn main():\n    println(\"hi\")\n")],
    );
    let mut session = Session::start(directory.path());

    let path = directory.path().join("dummy.mi");
    let refused = session.call(
        1,
        "view",
        json!({ "path": path.to_str().unwrap(), "around": "some text" }),
    );
    let error_msg = refused["error"]["message"]
        .as_str()
        .expect("error should have a message");

    // The error message should include the full parameter list for this method
    let expected_params =
        miri::cli::agent::schema::accepted("view").expect("view should have a schema");
    assert!(
        error_msg.contains(&expected_params),
        "error should list all accepted parameters: expected '{}' in: {}",
        expected_params,
        error_msg
    );

    session.finish();
}

#[test]
fn test_the_reference_client_works_against_the_real_binary() {
    // This test verifies that the reference Python client exercises all public
    // methods and can communicate with the binary correctly using proper framing.
    use std::process::Command;

    // Check if python3 is available; hard-fail if not to catch CI issues early
    let python_check = Command::new("python3").arg("--version").output();
    if python_check.is_err() {
        panic!("python3 is required to run this test but is not available");
    }

    let directory = project(
        "reference-client",
        &[(
            "fixture.mi",
            "fn double(x i32) i32\n    return x * 2\n\nfn main()\n    println(\"{double(21)}\")\n",
        )],
    );
    let fixture = directory.path().join("fixture.mi");
    let fixture_str = fixture.to_str().expect("fixture path is text");

    // Run reference client in full mode, which exercises all public methods
    let output = Command::new("python3")
        .arg(std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tools/agent_client.py"))
        .arg("--miri")
        .arg(assert_cmd::cargo_bin!("miri"))
        .arg("full")
        .arg(fixture_str)
        .current_dir(directory.path())
        .env(
            "MIRI_STDLIB_PATH",
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib"),
        )
        .output()
        .expect("reference client should run");

    assert!(
        output.status.success(),
        "reference client should exit successfully, stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let stdout = String::from_utf8(output.stdout).expect("output should be UTF-8");
    let json: Value =
        serde_json::from_str(&stdout).expect("reference client should output valid JSON");

    // Verify all methods returned results (not errors)
    assert_eq!(json["initialize"], json!(true), "initialize must succeed");
    assert_eq!(json["check"], json!(true), "check must succeed");
    assert_eq!(
        json["check_with_verify"],
        json!(true),
        "check with verify_mir must succeed"
    );
    assert_eq!(json["view"], json!(true), "view outline must succeed");
    assert_eq!(
        json["view_with_fn"],
        json!(true),
        "view with fn parameter must succeed"
    );
    assert_eq!(json["explain"], json!(true), "explain must succeed");
    assert_eq!(json["patch"], json!(true), "patch must succeed");
    assert_eq!(json["skills_get"], json!(true), "skills_get must succeed");
    assert_eq!(
        json["skills_get_named"],
        json!(true),
        "skills_get with name must succeed"
    );
}

#[test]
fn test_every_served_method_is_documented() {
    // This test ensures that the protocol documentation is complete:
    // for every method in the schema, there is a corresponding section in the docs.
    use std::path::PathBuf;

    let doc_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/agent-protocol.md");
    let doc_content =
        std::fs::read_to_string(&doc_path).expect("docs/agent-protocol.md should exist");

    let mut headings_found = 0;
    for method in miri::cli::agent::schema::METHODS {
        let heading = format!("### `{}`", method.method);
        assert!(
            doc_content.contains(&heading),
            "method {} should have a documentation section with heading: {}",
            method.method,
            heading
        );
        headings_found += 1;
    }

    assert!(
        headings_found > 0,
        "should have found documentation headings for every method"
    );
}

#[test]
fn test_each_documented_method_has_request_and_response_examples() {
    // This test verifies that the protocol documentation for each method
    // includes both a request example (marked by `Request:`) and a response
    // example (marked by a fenced `json` block).
    //
    // The request example is often inline text like `{ "path": "main.mi" }`,
    // while the response is always a JSON-RPC envelope in a code fence.
    // Additionally, all fenced JSON blocks are parsed to verify they are valid JSON.
    use std::path::PathBuf;

    let doc_path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("docs/agent-protocol.md");
    let doc_content =
        std::fs::read_to_string(&doc_path).expect("docs/agent-protocol.md should exist");

    for method in miri::cli::agent::schema::METHODS {
        let heading = format!("### `{}`", method.method);
        let heading_pos = doc_content
            .find(&heading)
            .unwrap_or_else(|| panic!("method {} missing documentation heading", method.method));

        // Find the next section heading (another ###) or end of file
        let section_start = heading_pos + heading.len();
        let section_end = doc_content[section_start..]
            .find("\n### ")
            .map(|pos| section_start + pos)
            .unwrap_or(doc_content.len());
        let section = &doc_content[section_start..section_end];

        // Check for request example (marked with "Request:")
        assert!(
            section.contains("Request:"),
            "method {} should document a request example with 'Request:' marker",
            method.method
        );

        // Extract and parse all JSON code blocks
        let mut json_count = 0;
        let mut current_pos = 0;
        while let Some(fence_start) = section[current_pos..].find("```json") {
            json_count += 1;
            let fence_start = current_pos + fence_start + 7;
            if let Some(fence_end) = section[fence_start..].find("```") {
                let json_text = section[fence_start..fence_start + fence_end].trim();
                // Parse the JSON to verify it is valid
                match serde_json::from_str::<serde_json::Value>(json_text) {
                    Ok(_) => {}
                    Err(err) => {
                        panic!(
                            "method {} has invalid JSON in documentation: {}",
                            method.method, err
                        )
                    }
                }
                current_pos = fence_start + fence_end;
            } else {
                break;
            }
        }

        assert!(
            json_count >= 1,
            "method {} should have at least one fenced JSON response block (found {})",
            method.method,
            json_count
        );
    }
}

#[test]
fn test_every_advertised_method_is_actually_served() {
    // Every method advertised by the handshake must actually answer requests,
    // not respond with -32601 (METHOD_NOT_FOUND). This test drives one minimal
    // request per served method to verify each is routed.
    let directory = project(
        "served-methods",
        &[("fixture.mi", "fn main():\n    println(\"ok\")\n")],
    );
    let path = directory.path().join("fixture.mi");
    let path_str = path.to_str().unwrap();
    let mut session = Session::start(directory.path());

    // Each served method gets a minimal valid request. The test only verifies
    // the method is routed (error code is not -32601); it does not verify
    // correctness of the result.
    let test_cases = vec![
        ("initialize", json!({})),
        ("check", json!({ "path": path_str })),
        ("explain", json!({ "code": "MER_TYP_030" })),
        ("fixPlan", json!({ "path": path_str })),
        ("fixApply", json!({ "path": path_str })),
        ("view", json!({ "path": path_str })),
        (
            "patch",
            json!({
                "path": path_str,
                "operations": []
            }),
        ),
        ("skillsGet", json!({})),
    ];

    for (index, (method, params)) in test_cases.iter().enumerate() {
        let response = session.call(index as i64 + 1, method, params.clone());
        let error_code = response["error"]["code"].clone();
        assert_ne!(
            error_code,
            json!(-32601),
            "method {} was advertised but returned -32601 (METHOD_NOT_FOUND): {}",
            method,
            response
        );
    }

    session.finish();
}
