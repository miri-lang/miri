// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The JSON-RPC 2.0 message shapes the agent transport speaks.
//!
//! This module holds the wire types and nothing else: no transport, no command
//! dispatch, no rendering. It sits in the inner diagnostics layer beside the
//! envelope it carries, so it names neither `crate::cli` nor `crate::error`.
//! The values it cannot know — the compiler's version, the set of methods a
//! build serves — are handed to it by the caller rather than read from an outer
//! layer.
//!
//! A request that the compiler answers at all answers with a
//! [`DiagnosticsEnvelope`](crate::diagnostics::json::DiagnosticsEnvelope) as its
//! result, exactly as the command line prints one. A program that fails to
//! compile is such an answer: the result is present and carries `ok: false`. The
//! error member is reserved for a request the compiler could not act on — one it
//! could not parse, a method it does not serve, a parameter it cannot read.

use serde::{Deserialize, Serialize};

/// The version of JSON-RPC these messages conform to.
pub const JSONRPC_VERSION: &str = "2.0";

/// The request could not be parsed as JSON.
pub const PARSE_ERROR: i32 = -32700;
/// The request parsed but is not a well-formed JSON-RPC request.
pub const INVALID_REQUEST: i32 = -32600;
/// The method is not one this build serves.
pub const METHOD_NOT_FOUND: i32 = -32601;
/// The method is served, but the parameters given do not fit it.
pub const INVALID_PARAMS: i32 = -32602;
/// The compiler could not act on a request it otherwise understood.
pub const INTERNAL_ERROR: i32 = -32603;
/// The request was withdrawn before it started.
///
/// This is the code LSP uses for the same condition. It sits in the range
/// JSON-RPC reserves for the protocol rather than for the server.
pub const REQUEST_CANCELLED: i32 = -32800;

/// A request identifier.
///
/// JSON-RPC lets a client identify a request with either a number or a string,
/// and requires the response to echo back what it was given, unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(untagged)]
pub enum RpcId {
    /// A numeric identifier.
    Number(i64),
    /// A string identifier.
    Text(String),
}

/// A request from a client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcRequest {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// Absent for a notification, which expects no response.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<RpcId>,
    /// The method being called.
    pub method: String,
    /// The method's arguments. Absent when the method takes none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl RpcRequest {
    /// Whether this message expects a response.
    ///
    /// A notification carries no identifier, and JSON-RPC forbids answering it.
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }

    /// Whether the message names the version of JSON-RPC this module speaks.
    pub fn has_supported_version(&self) -> bool {
        self.jsonrpc == JSONRPC_VERSION
    }
}

/// What went wrong with a request the compiler could not act on.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    /// The JSON-RPC error code.
    pub code: i32,
    /// A one-line description written for a person reading a log.
    pub message: String,
    /// Whatever a client needs to tell this failure from its neighbours.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

/// A response to a client.
///
/// Exactly one of `result` and `error` is present, which JSON-RPC requires.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcResponse {
    /// Always `"2.0"`.
    pub jsonrpc: String,
    /// The identifier of the request being answered. Null when the request
    /// could not be parsed far enough to recover one.
    pub id: Option<RpcId>,
    /// The answer, when the compiler acted on the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    /// Why the compiler did not act on the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl RpcResponse {
    /// Answer a request the compiler acted on.
    pub fn success(id: Option<RpcId>, result: serde_json::Value) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    /// Answer a request the compiler could not act on.
    pub fn failure(id: Option<RpcId>, code: i32, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC_VERSION.to_string(),
            id,
            result: None,
            error: Some(RpcError {
                code,
                message: message.into(),
                data: None,
            }),
        }
    }

    /// Attach the detail that tells this failure from its neighbours.
    pub fn with_data(mut self, data: serde_json::Value) -> Self {
        if let Some(error) = self.error.as_mut() {
            error.data = Some(data);
        }
        self
    }
}

/// What the compiler tells a client about itself when the session opens.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerInfo {
    /// The name of the program serving the session.
    pub name: String,
    /// The compiler's version, as its `--version` reports it.
    pub version: String,
    /// The version of the envelope every result on this session carries.
    ///
    /// A client compares this against the schema it was written for, so it is
    /// the same number `miri check --format json` emits.
    pub schema_version: u32,
}

/// What a session can do, so a client need not discover it by trying.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerCapabilities {
    /// The methods this build answers.
    pub methods: Vec<String>,
    /// The methods this build knows by name but does not yet answer.
    ///
    /// A client can tell a method that is coming from one it misspelled, and
    /// can say so to its user instead of reporting an unknown method.
    pub reserved_methods: Vec<String>,
    /// Whether a request can be withdrawn before it starts.
    pub cancellation: bool,
    /// JSON-Schema fragments for each method's parameters.
    ///
    /// Maps method name to a JSON-Schema object describing the method's
    /// accepted parameters and their types. Always present and populated
    /// by the server.
    pub method_schemas: serde_json::Map<String, serde_json::Value>,
}

/// The answer to the handshake that opens a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct InitializeResult {
    /// Who is serving the session.
    pub server_info: ServerInfo,
    /// What the session can do.
    pub capabilities: ServerCapabilities,
}
