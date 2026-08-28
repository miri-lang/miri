// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri agent` command: one compiler process answering many requests.
//!
//! A tool that drives the compiler pays the process start-up cost on every
//! invocation. This command pays it once and then answers JSON-RPC 2.0 requests
//! over stdin and stdout for as long as the caller keeps the session open,
//! framed the way a language server frames them — a `Content-Length` header, a
//! blank line, then that many bytes of JSON.
//!
//! Every method here answers with the envelope its command-line equivalent
//! prints. Nothing in this module decides what a diagnostic says, whether a
//! repair is safe, or which file a repair may rewrite; it reads a request,
//! calls the same code the command line calls, and writes the answer back. That
//! is what keeps the two transports telling one story.
//!
//! **stdout belongs to the protocol.** A stray line written to it would sit
//! inside a frame and desynchronise the stream, so everything this module says
//! to a human goes to stderr.
//!
//! **Cancellation reaches a request that has not started.** A reader thread
//! takes messages off stdin while the worker compiles, so a `$/cancelRequest`
//! that arrives during a long compile is seen immediately and withdraws any
//! queued request it names. The compile already running finishes and answers
//! normally: the pipeline has no cancellation point to unwind from, and adding
//! one would thread a token through every pass for a transport's benefit.

use std::collections::{HashSet, VecDeque};
use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};

use crate::cli::{check, explain, fix, patch, version_string, view};
use crate::diagnostics::rpc::{
    InitializeResult, RpcId, RpcRequest, RpcResponse, ServerCapabilities, ServerInfo,
    INVALID_PARAMS, INVALID_REQUEST, METHOD_NOT_FOUND, PARSE_ERROR, REQUEST_CANCELLED,
};

/// The methods this build answers.
const SERVED_METHODS: &[&str] = &[
    "initialize",
    "check",
    "explain",
    "fixPlan",
    "fixApply",
    "view",
    "patch",
];

/// The methods this build knows by name but does not yet answer.
///
/// Naming them is what lets a client tell a method that is coming from one it
/// misspelled. Each is the surface of a task that has not landed; a method
/// moves from here to [`SERVED_METHODS`] when its command exists.
const RESERVED_METHODS: &[&str] = &["tokens", "parse", "graph", "skillsGet", "targets", "doctor"];

/// The largest message body this session will accept.
///
/// `Content-Length` is the one number a client states before anything has
/// validated it, and it is what sizes the buffer the body is read into. Without
/// a ceiling, a header of a dozen digits asks for an allocation the process
/// cannot serve and dies on — one line of input ending the session. No real
/// request comes close to this bound: the largest thing the protocol carries is
/// a source file's worth of text.
const MAX_MESSAGE_BYTES: usize = 64 * 1024 * 1024;

/// The largest header section this session will read before giving up.
///
/// Headers are read a line at a time, and a stream that never sends a newline
/// would otherwise grow a buffer without end.
const MAX_HEADER_BYTES: u64 = 8 * 1024;

/// A message the reader took off stdin, or the reason it stopped.
enum Incoming {
    /// A request for the worker to answer.
    Request(Box<RpcRequest>),
    /// A message that could not be read as a JSON-RPC request.
    Malformed { id: Option<RpcId>, reason: String },
}

/// The most withdrawals this session will remember at once.
///
/// A withdrawal naming a request that never arrives is never claimed, so
/// without a bound a client could grow this set for as long as the session
/// lives simply by cancelling identifiers it never uses.
const MAX_REMEMBERED_CANCELLATIONS: usize = 1024;

/// The requests withdrawn by a `$/cancelRequest` and not yet claimed.
///
/// Bounded, and forgets the oldest first: a withdrawal that has gone unclaimed
/// the longest is the one least likely to still be waiting for its request.
#[derive(Default)]
struct Withdrawals {
    pending: HashSet<RpcId>,
    /// The order the pending identifiers were recorded in.
    ///
    /// An identifier stays here after being claimed and is skipped when it
    /// comes up for eviction, which keeps recording and claiming cheap.
    order: VecDeque<RpcId>,
}

impl Withdrawals {
    /// Remember that `id` was withdrawn.
    fn record(&mut self, id: RpcId) {
        if !self.pending.insert(id.clone()) {
            return;
        }
        self.order.push_back(id);
        while self.pending.len() > MAX_REMEMBERED_CANCELLATIONS {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.pending.remove(&oldest);
        }
    }

    /// Claim `id`, reporting whether it had been withdrawn.
    ///
    /// The identifier is forgotten as it is claimed: a client may reuse it once
    /// the request carrying it has been answered, and a withdrawal left behind
    /// would withdraw the reuse.
    fn claim(&mut self, id: &RpcId) -> bool {
        self.pending.remove(id)
    }
}

/// Requests withdrawn by a `$/cancelRequest` that has not been claimed yet.
type Cancellations = Arc<Mutex<Withdrawals>>;

/// Serve JSON-RPC requests until the client closes stdin.
///
/// Runs on the caller's thread, which the binary has already given a stack
/// large enough for the compiler's recursive passes.
pub fn run() -> std::io::Result<()> {
    let cancelled: Cancellations = Arc::new(Mutex::new(Withdrawals::default()));
    let (sender, receiver) = mpsc::channel();

    let reader_cancellations = Arc::clone(&cancelled);
    let reader = std::thread::Builder::new()
        .name("miri-agent-reader".to_string())
        .spawn(move || read_messages(std::io::stdin().lock(), sender, reader_cancellations))?;

    serve(receiver, &cancelled)?;

    // The reader has already stopped, or is about to: the channel it sends on
    // is closed once this function returns. Joining reports a panic in it
    // rather than letting the process exit as though the session ended
    // cleanly.
    match reader.join() {
        Ok(result) => result,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}

/// Take messages off `input` until it ends, forwarding each to the worker.
///
/// A `$/cancelRequest` is recorded here rather than forwarded, so that it
/// overtakes the queue instead of waiting behind the work it is withdrawing.
fn read_messages(
    input: impl BufRead,
    sender: Sender<Incoming>,
    cancelled: Cancellations,
) -> std::io::Result<()> {
    let mut input = input;
    loop {
        let body = match read_frame(&mut input)? {
            Some(body) => body,
            None => return Ok(()),
        };

        let message = match serde_json::from_str::<RpcRequest>(&body) {
            Ok(message) => message,
            Err(error) => {
                let malformed = Incoming::Malformed {
                    id: recover_id(&body),
                    reason: error.to_string(),
                };
                if sender.send(malformed).is_err() {
                    return Ok(());
                }
                continue;
            }
        };

        if message.method == "$/cancelRequest" {
            record_cancellation(&message, &cancelled);
            continue;
        }

        if sender.send(Incoming::Request(Box::new(message))).is_err() {
            return Ok(());
        }
    }
}

/// Note the request a `$/cancelRequest` withdraws.
///
/// A cancellation naming no request withdraws nothing. It is not an error: the
/// request it named may have been answered already.
fn record_cancellation(message: &RpcRequest, cancelled: &Cancellations) {
    let Some(params) = message.params.as_ref() else {
        return;
    };
    let Some(id) = params.get("id").and_then(parse_id) else {
        return;
    };
    // A poisoned lock means a thread panicked holding it. Losing the
    // withdrawal is the safe direction: the request it named simply runs and
    // answers, which is already what happens to a request that has started.
    if let Ok(mut withdrawals) = cancelled.lock() {
        withdrawals.record(id);
    }
}

/// Read one `id` value in either of the two shapes JSON-RPC allows.
fn parse_id(value: &serde_json::Value) -> Option<RpcId> {
    match value {
        serde_json::Value::Number(number) => number.as_i64().map(RpcId::Number),
        serde_json::Value::String(text) => Some(RpcId::Text(text.clone())),
        _ => None,
    }
}

/// Recover the identifier of a message that did not parse as a request.
///
/// JSON-RPC asks a server to answer a malformed request with the identifier it
/// carried when one can be read at all, so that a client can match the failure
/// to what it sent.
fn recover_id(body: &str) -> Option<RpcId> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value.get("id").and_then(parse_id))
}

/// Read one `Content-Length`-framed message.
///
/// Returns `None` at end of input.
fn read_frame<R: BufRead>(input: &mut R) -> std::io::Result<Option<String>> {
    let Some(length) = read_headers(input)? else {
        return Ok(None);
    };

    if length > MAX_MESSAGE_BYTES {
        // The stream cannot be resynchronised: the only thing saying where this
        // body ends is the number that was just rejected.
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "a frame declared {} bytes, over the {} byte limit",
                length, MAX_MESSAGE_BYTES
            ),
        ));
    }

    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    String::from_utf8(body)
        .map(Some)
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))
}

/// Read a frame's headers and return the body length they declare.
///
/// Returns `None` at end of input. A header this module does not know is
/// skipped rather than rejected, so that a client may send the `Content-Type`
/// a language server sends without being turned away.
fn read_headers<R: BufRead>(input: &mut R) -> std::io::Result<Option<usize>> {
    let mut length = None;
    loop {
        let mut line = String::new();
        let mut limited = <&mut R as std::io::Read>::take(&mut *input, MAX_HEADER_BYTES);
        let read = limited.read_line(&mut line)? as u64;
        if read == 0 {
            return Ok(None);
        }
        if read == MAX_HEADER_BYTES && !line.ends_with('\n') {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "a header line exceeded the length this session will read",
            ));
        }

        let line = line.trim_end_matches(['\r', '\n']);
        if line.is_empty() {
            return match length {
                Some(length) => Ok(Some(length)),
                None => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "a frame arrived without a Content-Length header",
                )),
            };
        }

        if let Some(value) = line.strip_prefix("Content-Length:") {
            length = value.trim().parse::<usize>().ok();
        }
    }
}

/// Answer messages until the reader stops sending them.
fn serve(receiver: Receiver<Incoming>, cancelled: &Cancellations) -> std::io::Result<()> {
    let stdout = std::io::stdout();
    for incoming in receiver {
        let response = match incoming {
            Incoming::Malformed { id, reason } => {
                Some(RpcResponse::failure(id, PARSE_ERROR, reason))
            }
            Incoming::Request(request) => answer(*request, cancelled),
        };

        if let Some(response) = response {
            write_frame(&mut stdout.lock(), &response)?;
        }
    }
    Ok(())
}

/// Answer one request, or decline to answer a notification.
fn answer(request: RpcRequest, cancelled: &Cancellations) -> Option<RpcResponse> {
    if request.is_notification() {
        return None;
    }
    let id = request.id.clone();

    if withdrawn(id.as_ref(), cancelled) {
        return Some(RpcResponse::failure(
            id,
            REQUEST_CANCELLED,
            "the request was withdrawn before it started",
        ));
    }

    if !request.has_supported_version() {
        return Some(RpcResponse::failure(
            id,
            INVALID_REQUEST,
            format!("unsupported jsonrpc version: {}", request.jsonrpc),
        ));
    }

    Some(dispatch(&request, id))
}

/// Whether a `$/cancelRequest` withdrew this request before it started.
fn withdrawn(id: Option<&RpcId>, cancelled: &Cancellations) -> bool {
    let Some(id) = id else {
        return false;
    };
    // As above, a poisoned lock resolves to "not withdrawn", so the request is
    // answered rather than dropped.
    match cancelled.lock() {
        Ok(mut withdrawals) => withdrawals.claim(id),
        Err(_) => false,
    }
}

/// Route a request to the command that answers it.
fn dispatch(request: &RpcRequest, id: Option<RpcId>) -> RpcResponse {
    match request.method.as_str() {
        "initialize" => serialize(id, &initialize()),
        "check" => run_check(request, id),
        "explain" => run_explain(request, id),
        "fixPlan" => run_fix(request, id, false),
        "fixApply" => run_fix(request, id, true),
        "view" => run_view(request, id),
        "patch" => run_patch(request, id),
        method if RESERVED_METHODS.contains(&method) => RpcResponse::failure(
            id,
            METHOD_NOT_FOUND,
            format!(
                "method '{}' is reserved and not served by this build",
                method
            ),
        )
        .with_data(serde_json::json!({ "reserved": true, "method": method })),
        method => RpcResponse::failure(id, METHOD_NOT_FOUND, format!("unknown method: {}", method))
            .with_data(serde_json::json!({ "reserved": false, "method": method })),
    }
}

/// Describe this build to a client opening a session.
fn initialize() -> InitializeResult {
    InitializeResult {
        server_info: ServerInfo {
            name: "miri".to_string(),
            version: version_string(),
            schema_version: crate::diagnostics::json::SCHEMA_VERSION,
        },
        capabilities: ServerCapabilities {
            methods: SERVED_METHODS.iter().map(|m| m.to_string()).collect(),
            reserved_methods: RESERVED_METHODS.iter().map(|m| m.to_string()).collect(),
            cancellation: true,
        },
    }
}

/// Type-check the file the request names.
fn run_check(request: &RpcRequest, id: Option<RpcId>) -> RpcResponse {
    let Some(path) = required_path(request) else {
        return missing_path(id);
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return unreadable(id, &path, &error),
    };

    let verify_mir = request
        .params
        .as_ref()
        .and_then(|params| params.get("verifyMir"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    serialize(id, &check::check(&path, &source, verify_mir).envelope)
}

/// Explain the diagnostic code the request names.
fn run_explain(request: &RpcRequest, id: Option<RpcId>) -> RpcResponse {
    let Some(code) = string_param(request, "code") else {
        return RpcResponse::failure(
            id,
            INVALID_PARAMS,
            "explain needs a `code` parameter naming a diagnostic code",
        );
    };
    serialize(id, &explain::envelope(&code))
}

/// Read part of the file the request names.
///
/// The shape follows the command line: a `fn` parameter reads one function,
/// optionally narrowed by `around`, and its absence reads the file's outline.
fn run_view(request: &RpcRequest, id: Option<RpcId>) -> RpcResponse {
    let Some(path) = required_path(request) else {
        return missing_path(id);
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return unreadable(id, &path, &error),
    };

    let around = string_param(request, "around");
    let shape = match string_param(request, "fn") {
        Some(name) => view::Shape::Function { name, around },
        None if around.is_some() => {
            return RpcResponse::failure(
                id,
                INVALID_PARAMS,
                "view needs a `fn` parameter for `around` to narrow",
            )
        }
        None => view::Shape::Outline,
    };

    serialize(id, &view::view(&path, &source, &shape).envelope)
}

/// Apply edits to the file the request names and re-check what they produced.
///
/// The edits arrive as a list so that a batch costs one round trip and one
/// check, which is the whole reason this method exists rather than a caller
/// writing the file itself and asking for a check afterwards.
fn run_patch(request: &RpcRequest, id: Option<RpcId>) -> RpcResponse {
    let Some(path) = required_path(request) else {
        return missing_path(id);
    };

    let operations = match patch_operations(request) {
        Ok(operations) => operations,
        Err(reason) => return RpcResponse::failure(id, INVALID_PARAMS, reason),
    };

    let mode = match string_param(request, "mode").as_deref() {
        None | Some("apply") => patch::Mode::Apply,
        Some("checkOnly") => patch::Mode::CheckOnly,
        Some("dryRun") => patch::Mode::DryRun,
        Some(other) => {
            return RpcResponse::failure(
                id,
                INVALID_PARAMS,
                format!(
                    "unknown mode '{}'; expected apply, checkOnly or dryRun",
                    other
                ),
            )
        }
    };

    let expect_sha = string_param(request, "expectSha");
    serialize(
        id,
        &patch::patch(&path, &operations, expect_sha.as_deref(), mode).envelope,
    )
}

/// Read the `operations` list every patch request carries.
fn patch_operations(request: &RpcRequest) -> Result<Vec<patch::Operation>, String> {
    let entries = request
        .params
        .as_ref()
        .and_then(|params| params.get("operations"))
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| "patch needs an `operations` array".to_string())?;

    entries.iter().map(patch_operation).collect()
}

/// Read one entry of a patch request's `operations` list.
fn patch_operation(entry: &serde_json::Value) -> Result<patch::Operation, String> {
    let text = |name: &str| entry.get(name).and_then(serde_json::Value::as_str);
    let function = text("function")
        .ok_or_else(|| "each operation needs a `function`".to_string())?
        .to_string();

    match (text("old"), text("new"), text("body")) {
        (Some(old), Some(new), None) => Ok(patch::Operation {
            function,
            edit: patch::Edit::Anchored {
                old: old.to_string(),
                new: new.to_string(),
            },
        }),
        (None, None, Some(body)) => Ok(patch::Operation {
            function,
            edit: patch::Edit::Body {
                text: body.to_string(),
            },
        }),
        _ => {
            Err("each operation carries either `old` with `new`, or `body` on its own".to_string())
        }
    }
}

/// Report the repairs for the file the request names, and optionally write them.
///
/// `fixApply` has no terminal to confirm at, so the caller says outright
/// whether a repair the compiler classes as risky may be written. The default
/// is that it may not.
fn run_fix(request: &RpcRequest, id: Option<RpcId>, apply: bool) -> RpcResponse {
    let Some(path) = required_path(request) else {
        return missing_path(id);
    };

    let source = match std::fs::read_to_string(&path) {
        Ok(source) => source,
        Err(error) => return unreadable(id, &path, &error),
    };

    let (diagnostics, ok) = fix::diagnose(&path, &source);
    if !apply {
        return serialize(id, &fix::plan_envelope(&diagnostics, ok));
    }

    let allow_risky = request
        .params
        .as_ref()
        .and_then(|params| params.get("allowRisky"))
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);

    let report = fix::apply(&path, &source, &diagnostics, allow_risky);
    serialize(id, &fix::apply_envelope(&report, &diagnostics))
}

/// Read the `path` parameter every file-taking method requires.
fn required_path(request: &RpcRequest) -> Option<PathBuf> {
    string_param(request, "path").map(PathBuf::from)
}

/// Report a request that named no file to work on.
fn missing_path(id: Option<RpcId>) -> RpcResponse {
    RpcResponse::failure(
        id,
        INVALID_PARAMS,
        "this method needs a `path` parameter naming a source file",
    )
}

/// Read one string-valued parameter.
fn string_param(request: &RpcRequest, name: &str) -> Option<String> {
    request
        .params
        .as_ref()?
        .get(name)?
        .as_str()
        .map(str::to_string)
}

/// Report a file the compiler was asked to read and could not.
///
/// This is a failure of the request rather than a verdict on a program, so it
/// is an error member rather than an envelope saying the file does not compile.
fn unreadable(id: Option<RpcId>, path: &Path, error: &std::io::Error) -> RpcResponse {
    RpcResponse::failure(
        id,
        INVALID_PARAMS,
        format!("could not read {}: {}", path.display(), error),
    )
    .with_data(serde_json::json!({ "path": path.display().to_string() }))
}

/// Answer with a value, or report that the value could not be serialized.
fn serialize<T: serde::Serialize>(id: Option<RpcId>, value: &T) -> RpcResponse {
    match serde_json::to_value(value) {
        Ok(value) => RpcResponse::success(id, value),
        Err(error) => RpcResponse::failure(
            id,
            crate::diagnostics::rpc::INTERNAL_ERROR,
            format!("could not serialize the answer: {}", error),
        ),
    }
}

/// Write one `Content-Length`-framed response.
fn write_frame(output: &mut impl Write, response: &RpcResponse) -> std::io::Result<()> {
    let body = serde_json::to_string(response).unwrap_or_else(|error| {
        // A response is plain data, so this cannot realistically fail. Degrade
        // to a well-formed frame rather than desynchronising the stream.
        format!(
            r#"{{"jsonrpc":"2.0","id":null,"error":{{"code":{},"message":"could not serialize the response: {}"}}}}"#,
            crate::diagnostics::rpc::INTERNAL_ERROR,
            error
        )
    });

    write!(output, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
    output.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufReader;

    fn frame(body: &str) -> String {
        format!("Content-Length: {}\r\n\r\n{}", body.len(), body)
    }

    #[test]
    fn test_a_frame_is_read_back_whole() {
        let wire = frame(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
        let body = read_frame(&mut BufReader::new(wire.as_bytes()))
            .expect("the frame is well formed")
            .expect("the input is not empty");
        assert_eq!(body, r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#);
    }

    #[test]
    fn test_two_frames_do_not_bleed_into_each_other() {
        // The body length, not a delimiter, is what ends a frame. A body
        // containing the header text of the next frame must not truncate it.
        let wire = format!(
            "{}{}",
            frame(r#"{"a":"Content-Length: 9"}"#),
            frame(r#"{"b":2}"#)
        );
        let mut input = BufReader::new(wire.as_bytes());

        let first = read_frame(&mut input)
            .expect("well formed")
            .expect("present");
        let second = read_frame(&mut input)
            .expect("well formed")
            .expect("present");

        assert_eq!(first, r#"{"a":"Content-Length: 9"}"#);
        assert_eq!(second, r#"{"b":2}"#);
    }

    #[test]
    fn test_a_header_the_server_does_not_know_is_skipped() {
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#;
        let wire = format!(
            "Content-Type: application/vscode-jsonrpc\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let read = read_frame(&mut BufReader::new(wire.as_bytes()))
            .expect("an unknown header is not a failure")
            .expect("present");
        assert_eq!(read, body);
    }

    #[test]
    fn test_end_of_input_ends_the_session() {
        let read =
            read_frame(&mut BufReader::new(&b""[..])).expect("an empty stream is not an error");
        assert!(read.is_none(), "end of input must end the session");
    }

    #[test]
    fn test_a_frame_without_a_length_is_refused() {
        let wire = "Content-Type: application/json\r\n\r\n{}";
        let read = read_frame(&mut BufReader::new(wire.as_bytes()));
        assert!(
            read.is_err(),
            "a frame whose length is unknown cannot be read without desynchronising the stream"
        );
    }

    #[test]
    fn test_an_identifier_survives_a_body_that_is_not_a_valid_request() {
        // A body that parses as JSON but does not fit a request — here the
        // method is a number — still names the request it came from, so the
        // failure can be matched to what the client sent.
        assert_eq!(
            recover_id(r#"{"jsonrpc":"2.0","id":7,"method":42}"#),
            Some(RpcId::Number(7))
        );
        assert_eq!(
            recover_id(r#"{"jsonrpc":"2.0","id":"seven","method":42}"#),
            Some(RpcId::Text("seven".to_string()))
        );
    }

    #[test]
    fn test_an_identifier_is_not_invented_for_a_body_that_is_not_json() {
        // JSON-RPC requires a null identifier when the request could not be
        // parsed at all: nothing in it can be trusted to name a request.
        assert_eq!(recover_id(r#"{"jsonrpc":"2.0","id":7,"method":}"#), None);
        assert_eq!(recover_id("not json at all"), None);
    }

    #[test]
    fn test_a_frame_declaring_more_than_the_limit_is_refused_without_allocating_it() {
        // The length is the one number a client states before anything checks
        // it, and it sizes the buffer the body is read into. Taken at face
        // value, a dozen digits ask for an allocation the process dies on.
        let wire = format!("Content-Length: {}\r\n\r\n", usize::MAX);

        let refused = read_frame(&mut BufReader::new(wire.as_bytes()));

        let error = refused.expect_err("a body larger than the limit must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error.to_string().contains("limit"),
            "the refusal should say what was exceeded: {}",
            error
        );
    }

    #[test]
    fn test_a_frame_at_the_limit_is_still_read() {
        // The bound must reject the absurd without rejecting the merely large.
        let body = format!("{{\"padding\":\"{}\"}}", "x".repeat(4096));
        let wire = frame(&body);

        let read = read_frame(&mut BufReader::new(wire.as_bytes()))
            .expect("a body well inside the limit is fine")
            .expect("present");

        assert_eq!(read, body);
    }

    #[test]
    fn test_a_header_line_that_never_ends_is_refused() {
        // A stream that sends header bytes and never a newline would otherwise
        // grow a buffer for as long as it kept sending.
        let wire = "X".repeat((MAX_HEADER_BYTES as usize) + 1024);

        let refused = read_frame(&mut BufReader::new(wire.as_bytes()));

        let error = refused.expect_err("an unbounded header line must be refused");
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }

    #[test]
    fn test_a_withdrawal_is_forgotten_once_it_is_claimed() {
        let mut withdrawals = Withdrawals::default();
        withdrawals.record(RpcId::Number(1));

        assert!(
            withdrawals.claim(&RpcId::Number(1)),
            "the first claim takes it"
        );
        assert!(
            !withdrawals.claim(&RpcId::Number(1)),
            "a client may reuse the identifier afterwards"
        );
    }

    #[test]
    fn test_unclaimed_withdrawals_do_not_accumulate_without_end() {
        // A withdrawal naming a request that never arrives is never claimed. A
        // client could otherwise grow this set for the life of the session.
        let mut withdrawals = Withdrawals::default();
        for id in 0..(MAX_REMEMBERED_CANCELLATIONS as i64 * 3) {
            withdrawals.record(RpcId::Number(id));
        }

        assert!(
            withdrawals.pending.len() <= MAX_REMEMBERED_CANCELLATIONS,
            "the set grew to {} entries, past the {} it may hold",
            withdrawals.pending.len(),
            MAX_REMEMBERED_CANCELLATIONS
        );
    }

    #[test]
    fn test_the_most_recent_withdrawals_are_the_ones_kept() {
        // Forgetting the oldest first keeps the withdrawals most likely to
        // still be waiting for their request.
        let mut withdrawals = Withdrawals::default();
        for id in 0..(MAX_REMEMBERED_CANCELLATIONS as i64 + 1) {
            withdrawals.record(RpcId::Number(id));
        }

        assert!(
            !withdrawals.claim(&RpcId::Number(0)),
            "the oldest withdrawal should have been forgotten"
        );
        assert!(
            withdrawals.claim(&RpcId::Number(MAX_REMEMBERED_CANCELLATIONS as i64)),
            "the newest withdrawal should still be remembered"
        );
    }

    #[test]
    fn test_recording_the_same_withdrawal_twice_holds_one_entry() {
        let mut withdrawals = Withdrawals::default();
        withdrawals.record(RpcId::Number(4));
        withdrawals.record(RpcId::Number(4));

        assert_eq!(withdrawals.pending.len(), 1);
        assert_eq!(
            withdrawals.order.len(),
            1,
            "the order must not grow a duplicate"
        );
    }

    #[test]
    fn test_every_reserved_method_is_distinct_from_a_served_one() {
        for reserved in RESERVED_METHODS {
            assert!(
                !SERVED_METHODS.contains(reserved),
                "{} is listed as both served and reserved",
                reserved
            );
        }
    }
}
