// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A drop hook may not let its receiver escape.
//!
//! While a hook runs, its receiver's count is immortal, so the hook's own
//! retains and releases cannot free the instance under it. Whatever kept the
//! receiver past the hook — a value built of it, a field or an element it is
//! stored in, a closure capturing it, a callee keeping it — would hold an
//! instance freed the moment the hook returns. So the hook body keeping its
//! receiver is refused where the hook keeps it.
//!
//! The answer is the hook's escape summary, the one the kept-parameter
//! decision reads for every other method: the receiver is slot 0, and the
//! first place the body keeps it, with the chain of callees it passes
//! through, is what the refusal shows. Reading the receiver's fields and
//! lending it to a callee that does not keep it are not escapes. A call
//! through a local holding a function value keeps what the body it runs
//! keeps, and what it is handed when the walk cannot tell which body runs.

use std::collections::HashMap;

use crate::ast::statement::{Statement, DROP_HOOK_NAME};
use crate::diagnostics::DiagnosticCode;
use crate::error::type_error::{TypeError, TypeErrorKind};

use super::diagnostics::shown_type_name;
use super::escape_analysis::{EscapeSummary, FunctionId};
use super::use_after_move::consumed_because_chain;

/// The position a method's escape summary numbers its receiver at.
const RECEIVER_SLOT: usize = 0;

/// The refusal of the drop hook `statement` of `owner` keeping its receiver,
/// at the first place its body keeps it; `None` when the hook's summary
/// keeps no receiver. A place outside the program's own file is not one a
/// report can point at, so the refusal points at the hook instead.
pub(crate) fn receiver_escape_refusal(
    owner: &str,
    statement: &Statement,
    summaries: &HashMap<FunctionId, EscapeSummary>,
) -> Option<TypeError> {
    let hook = FunctionId::method(owner, DROP_HOOK_NAME);
    let summary = summaries.get(&hook)?;
    if !summary.directly_escapes(RECEIVER_SLOT) {
        return None;
    }
    let site = summary.escape_next_hops.get(&RECEIVER_SLOT);
    let span = site.and_then(|site| site.span).unwrap_or(statement.span);
    let owner = shown_type_name(owner);
    Some(TypeError {
        kind: TypeErrorKind::Coded {
            code: DiagnosticCode::OwnHookReceiverEscapes,
            message: format!(
                "'{owner}.drop' keeps 'self', but the instance is freed when its drop hook \
                 returns, so nothing may hold it after"
            ),
            help: Some(
                "release what self holds explicitly; to keep a value past the hook, copy the \
                 plain data out of self's fields"
                    .to_string(),
            ),
            expected: None,
            actual: None,
        },
        span,
        source_override: None,
        repair: None,
        related: consumed_because_chain(summaries, &hook, RECEIVER_SLOT),
    })
}
