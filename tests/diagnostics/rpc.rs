// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Wire-shape tests for the JSON-RPC message types.
//!
//! These pin the parts of the encoding a client depends on and which a
//! refactor could quietly change: that exactly one of `result` and `error` is
//! present, that an identifier comes back in the shape it was sent, and that
//! the error numbers are the ones JSON-RPC assigns.

use miri::diagnostics::rpc::{
    RpcId, RpcRequest, RpcResponse, INTERNAL_ERROR, INVALID_PARAMS, INVALID_REQUEST,
    METHOD_NOT_FOUND, PARSE_ERROR, REQUEST_CANCELLED,
};
use serde_json::{json, Value};

fn encode(response: &RpcResponse) -> Value {
    serde_json::to_value(response).expect("a response is plain data and should serialize")
}

#[test]
fn test_a_success_carries_a_result_and_no_error_member() {
    // JSON-RPC requires exactly one of the two. A response carrying both, or
    // carrying `error: null`, is not a valid response.
    let encoded = encode(&RpcResponse::success(
        Some(RpcId::Number(1)),
        json!({ "ok": true }),
    ));

    assert_eq!(encoded["jsonrpc"], json!("2.0"));
    assert_eq!(encoded["result"], json!({ "ok": true }));
    assert!(
        encoded.get("error").is_none(),
        "a success must not carry an error member: {}",
        encoded
    );
}

#[test]
fn test_a_failure_carries_an_error_and_no_result_member() {
    let encoded = encode(&RpcResponse::failure(
        Some(RpcId::Number(1)),
        METHOD_NOT_FOUND,
        "unknown method: nope",
    ));

    assert_eq!(encoded["error"]["code"], json!(METHOD_NOT_FOUND));
    assert_eq!(encoded["error"]["message"], json!("unknown method: nope"));
    assert!(
        encoded.get("result").is_none(),
        "a failure must not carry a result member: {}",
        encoded
    );
}

#[test]
fn test_an_identifier_returns_in_the_shape_it_arrived() {
    // A client chooses between a number and a string, and matches the response
    // against what it sent. Coercing one into the other would break that match.
    let numeric = encode(&RpcResponse::success(Some(RpcId::Number(7)), json!(null)));
    let textual = encode(&RpcResponse::success(
        Some(RpcId::Text("seven".to_string())),
        json!(null),
    ));

    assert_eq!(numeric["id"], json!(7));
    assert_eq!(textual["id"], json!("seven"));
}

#[test]
fn test_a_response_to_an_unreadable_request_names_a_null_identifier() {
    // JSON-RPC requires a null identifier when none could be read, and the
    // member must be present rather than omitted.
    let encoded = encode(&RpcResponse::failure(None, PARSE_ERROR, "not JSON"));

    assert!(
        encoded.as_object().expect("an object").contains_key("id"),
        "the identifier member must be present: {}",
        encoded
    );
    assert_eq!(encoded["id"], json!(null));
}

#[test]
fn test_attached_data_reaches_the_client() {
    let encoded = encode(
        &RpcResponse::failure(Some(RpcId::Number(1)), METHOD_NOT_FOUND, "reserved")
            .with_data(json!({ "reserved": true, "method": "view" })),
    );

    assert_eq!(encoded["error"]["data"]["reserved"], json!(true));
    assert_eq!(encoded["error"]["data"]["method"], json!("view"));
}

#[test]
fn test_data_attached_to_a_success_is_dropped_rather_than_misplaced() {
    // `with_data` describes a failure. Attaching it to a success must not
    // invent an error member, which would make the response invalid.
    let encoded =
        encode(&RpcResponse::success(Some(RpcId::Number(1)), json!(1)).with_data(json!("x")));

    assert!(
        encoded.get("error").is_none(),
        "a success must stay a success: {}",
        encoded
    );
    assert_eq!(encoded["result"], json!(1));
}

#[test]
fn test_the_error_numbers_are_the_ones_the_protocol_assigns() {
    // These are read by every client and are not ours to renumber.
    assert_eq!(PARSE_ERROR, -32700);
    assert_eq!(INVALID_REQUEST, -32600);
    assert_eq!(METHOD_NOT_FOUND, -32601);
    assert_eq!(INVALID_PARAMS, -32602);
    assert_eq!(INTERNAL_ERROR, -32603);
    assert_eq!(REQUEST_CANCELLED, -32800);
}

#[test]
fn test_a_request_without_an_identifier_is_a_notification() {
    let notification: RpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","method":"initialize"}"#)
            .expect("a notification is a valid request object");
    let request: RpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
            .expect("a request is a valid request object");

    assert!(notification.is_notification());
    assert!(!request.is_notification());
}

#[test]
fn test_a_request_naming_another_protocol_version_is_recognised_as_such() {
    let older: RpcRequest = serde_json::from_str(r#"{"jsonrpc":"1.0","id":1,"method":"check"}"#)
        .expect("the message parses; the version is what is wrong with it");

    assert!(!older.has_supported_version());
}

#[test]
fn test_a_request_parses_with_or_without_parameters() {
    let bare: RpcRequest =
        serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#)
            .expect("a method taking no parameters may omit them");
    assert!(bare.params.is_none());

    let carried: RpcRequest = serde_json::from_str(
        r#"{"jsonrpc":"2.0","id":1,"method":"check","params":{"path":"a.mi"}}"#,
    )
    .expect("parameters parse");
    assert_eq!(carried.params.expect("present")["path"], json!("a.mi"));
}
