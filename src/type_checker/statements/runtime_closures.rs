// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Holding a `runtime` declaration's closure parameters to what the runtime
//! does with them.
//!
//! The runtime calls a closure it is handed through a trampoline the compiler
//! generates from the closure's declared type, and calls it in exactly one
//! shape: the arity, the `int` it reads the answer as, and the collection
//! whose element slots it passes, which [`crate::runtime_fns::closure_contract`]
//! records per entry point. A declaration that promises another shape would
//! have the trampoline read past an element slot or write past the answer, so
//! it is refused where it is written.

use crate::ast::common::Parameter;
use crate::ast::types::{FunctionTypeData, Type, TypeKind};
use crate::diagnostics::DiagnosticCode;
use crate::runtime_fns::{closure_contract, ClosureContract};
use crate::type_checker::{resolve_type_name, TypeChecker};

impl TypeChecker {
    /// Refuse each parameter of the runtime function `name` whose declared
    /// type disagrees with the closure the runtime calls through it, or calls
    /// none through.
    pub(crate) fn check_runtime_closure_params(&mut self, name: &str, params: &[Parameter]) {
        for (position, param) in params.iter().enumerate() {
            let function = resolve_type_name(&param.typ).and_then(|ty| {
                if let TypeKind::Function(data) = ty.kind {
                    Some(*data)
                } else {
                    None
                }
            });
            let refusal = match (closure_contract(name, position), function) {
                (None, None) => None,
                (None, Some(_)) => Some(format!(
                    "`{name}` calls no closure through `{}`, so it cannot be declared as a function",
                    param.name
                )),
                (Some(_), None) => Some(format!(
                    "`{name}` calls `{}` as a closure, so it must be declared as a function",
                    param.name
                )),
                (Some(contract), Some(function)) => {
                    closure_refusal(name, params, (param, &function), contract)
                }
            };
            if let Some(message) = refusal {
                self.report_error(
                    DiagnosticCode::TypFunctionSignature,
                    message,
                    param.typ.span,
                );
            }
        }
    }
}

/// Why the closure parameter `param`, declared as `function`, breaks the
/// runtime's `contract` for it; `None` when it keeps it.
fn closure_refusal(
    name: &str,
    params: &[Parameter],
    (param, function): (&Parameter, &FunctionTypeData),
    contract: ClosureContract,
) -> Option<String> {
    let closure = &param.name;
    if function.params.len() != contract.arity {
        return Some(format!(
            "`{name}` calls `{closure}` with {} arguments, but `{closure}` is declared to take {}",
            contract.arity,
            function.params.len()
        ));
    }
    let answer = function.return_type.as_deref().and_then(resolve_type_name);
    if !answer.as_ref().is_some_and(|ty| ty.kind == TypeKind::Int) {
        let declared = answer.map_or_else(|| "nothing".to_string(), |ty| ty.to_string());
        return Some(format!(
            "`{name}` reads what `{closure}` answers as an `int`, but `{closure}` is declared to answer `{declared}`"
        ));
    }
    let collection = params.get(contract.elements_of?)?;
    let element =
        resolve_type_name(&collection.typ).and_then(|ty| ty.kind.sequence_element_kind().cloned());
    let mismatch = function
        .params
        .iter()
        .filter_map(|argument| resolve_type_name(&argument.typ))
        .find(|argument| element.as_ref() != Some(&argument.kind))?;
    Some(element_refusal(
        name,
        (closure, &mismatch),
        collection,
        element,
    ))
}

/// The refusal of a closure `closure` taking `argument` where the runtime
/// passes it elements of `collection`, which hold `element`.
fn element_refusal(
    name: &str,
    (closure, argument): (&str, &Type),
    collection: &Parameter,
    element: Option<TypeKind>,
) -> String {
    let holds = element.map_or_else(
        || "not a sequence's elements".to_string(),
        |kind| format!("`{kind}`"),
    );
    format!(
        "`{name}` calls `{closure}` with elements of `{}`, which are {holds}, but `{closure}` takes `{argument}`",
        collection.name
    )
}
