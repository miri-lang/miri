// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether an argument to an `out` parameter names a place the call may
//! take, and the refusal when it does not.
//!
//! A call writes only a place rooted at a `var`, an `out` parameter or a
//! method's own `self`, reached through fields and elements, and takes each
//! such root once. Every refusal names the place it was handed and the
//! binding it starts at, states that rule, and gives a repair that compiles:
//! the `var` to declare, the parameter to declare `out`, the variable to bind
//! a call's result to, or the copy into a `var` that is passed and stored
//! back, the slower spelling that also works where no place path does.

use std::collections::HashMap;

use super::{OutSite, OutTaker};
use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::place_path::{path_links, path_root, PathLink};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::Context;
use crate::type_checker::diagnostics::spelled;
use crate::type_checker::place_reads::{index_spelling, place_spelling};
use crate::type_checker::writable_places::{let_to_var_repair, ImmutableBinding, Refusal};
use crate::type_checker::TypeChecker;

/// The rule every refusal of a root states.
const PLACE_RULE: &str =
    "a call writes only a place rooted at a 'var', an 'out' parameter or 'self'";

/// The places one call takes and the values it lends beside them, as its
/// arguments are checked.
#[derive(Default)]
pub(super) struct CallPlaces<'e> {
    /// Each root taken so far, with the place it was taken through, as the
    /// program spells it.
    roots: HashMap<String, String>,
    /// Each place handed to an `out` parameter and accepted, with the
    /// parameter's name.
    taken: Vec<(String, &'e Expression)>,
    /// Each argument handed to a parameter that is not `out`.
    lent: Vec<&'e Expression>,
}

impl<'e> CallPlaces<'e> {
    /// Records `arg`, handed to a parameter the callee only reads.
    pub(super) fn lend(&mut self, arg: &'e Expression) {
        self.lent.push(arg);
    }
}

impl TypeChecker {
    /// Validates that the argument `arg` to the `out` parameter `param_name`
    /// is a place the call may take: a `var`, an `out` parameter or a
    /// method's receiver, or a path of fields and elements from one, taken
    /// once per call ([`TypeChecker::writable_place`]).
    pub(super) fn validate_out_argument_place<'e>(
        &mut self,
        param_name: &str,
        arg: &'e Expression,
        places: &mut CallPlaces<'e>,
        site: OutSite<'_>,
        context: &Context,
    ) {
        if matches!(&arg.node, ExpressionKind::Identifier(name, _) if name == "self") {
            self.refuse_replacing_an_instance(site.span, context);
            return;
        }
        let place = match self.writable_place(arg, context) {
            Ok(place) => place,
            Err(refusal) => {
                return self.refuse_out_argument(param_name, arg, refusal, site.span, context)
            }
        };
        let spelled = spelled_argument(arg);
        match (place.steps.is_empty(), site.taker) {
            (true, OutTaker::Lowered | OutTaker::Builtin) => {}
            (false, OutTaker::Lowered | OutTaker::Builtin) if context.is_device_code() => {
                self.refuse_out_path_in_device_code(param_name, &spelled, site.span)
            }
            (false, OutTaker::Builtin) => {
                self.refuse_out_path_to_builtin(param_name, &spelled, site.span)
            }
            (false, OutTaker::Lowered) => {}
        }
        let root = place.root_name();
        match places.roots.get(root) {
            Some(first) => self.refuse_root_taken_twice(root, (first, &spelled), site.span),
            None => {
                places.roots.insert(root.to_string(), spelled);
                places.taken.push((param_name.to_string(), arg));
            }
        }
    }

    /// Refuses each place a call takes that a value it lends beside it holds:
    /// another argument, or `receiver`, the value a method call is made on,
    /// naming the place's root or a path toward the place. Two elements of
    /// one collection cannot be proved different, so a path through
    /// `v[j]` toward `v[i].conn` holds it. The callee would read the place
    /// through that value while it writes it, so the call lends a value
    /// containing a place it writes, which is refused.
    pub(super) fn refuse_lent_holders(
        &mut self,
        places: &CallPlaces<'_>,
        receiver: Option<&Expression>,
    ) {
        let lent = || {
            let arguments = places.lent.iter().map(|arg| (*arg, Lent::Argument));
            (receiver
                .map(|receiver| (receiver, Lent::Receiver))
                .into_iter())
            .chain(arguments)
        };
        for (param_name, taken) in &places.taken {
            if let Some(holder) = lent().find(|(value, _)| holds_place(value, taken)) {
                self.refuse_lent_holder(param_name, taken, holder);
            }
        }
    }

    /// Refuses the place `taken`, handed to the `out` parameter
    /// `param_name`, beside `holder`, a value holding it the call lends.
    fn refuse_lent_holder(
        &mut self,
        param_name: &str,
        taken: &Expression,
        (holder, lent_as): (&Expression, Lent),
    ) {
        let place = spelled_argument(taken);
        let holder_spelled = spelled_argument(holder);
        let lends = match lent_as {
            Lent::Argument => format!("the call also lends '{holder_spelled}'"),
            Lent::Receiver => format!("the call is made on '{holder_spelled}'"),
        };
        let message = format!(
            "'out' parameter '{param_name}' is handed '{place}', and {lends}, a value holding that place: a call may not lend a value containing a place it writes"
        );
        let help = copied_in_and_out_help(&place);
        self.report_error_with_help(
            DiagnosticCode::TypOutParameterMisuse,
            message,
            taken.span,
            help,
        );
    }

    /// Reports the argument `arg` to the `out` parameter `param_name`, which
    /// names no place a write may land in for the reason `refusal` gives.
    fn refuse_out_argument(
        &mut self,
        param_name: &str,
        arg: &Expression,
        refusal: Refusal<'_>,
        span: Span,
        context: &Context,
    ) {
        let place = spelled_argument(arg);
        let rest = steps_below(arg, None);
        let (code, message, help) = match refusal {
            Refusal::NotAPlace => not_a_place(param_name, arg),
            Refusal::ImmutableRoot(name, binding) => (
                DiagnosticCode::TypImmutabilityViolation,
                immutable_root_message(param_name, &place, name, binding),
                immutable_root_help(name, binding, (&place, &rest), context),
            ),
            Refusal::ImmutableField(field) => (
                DiagnosticCode::TypImmutabilityViolation,
                format!(
                    "'out' parameter '{param_name}' is handed '{place}', a path through the field '{field}', which its class declares with 'let'"
                ),
                format!("declare the field '{field}' with 'var' in its class, then pass '{place}'"),
            ),
            Refusal::Entry(entry) => {
                let rest = steps_below(arg, Some(entry));
                entry_refusal(param_name, (&place, &rest), entry)
            }
            Refusal::DeviceResident(name) => {
                device_resident_refusal(param_name, (&place, &rest), name)
            }
            Refusal::TupleSlot(tuple) => tuple_slot_refusal(param_name, &place, tuple),
        };
        let repair = match refusal {
            Refusal::ImmutableRoot(name, ImmutableBinding::Let) => let_to_var_repair(name, context),
            Refusal::ImmutableRoot(
                _,
                ImmutableBinding::LentParameter
                | ImmutableBinding::Constant
                | ImmutableBinding::LoopBinding,
            )
            | Refusal::NotAPlace
            | Refusal::ImmutableField(_)
            | Refusal::Entry(_)
            | Refusal::DeviceResident(_)
            | Refusal::TupleSlot(_) => None,
        };
        self.report_error_with_help_and_optional_repair(code, message, span, help, repair);
    }

    /// Refuses the field or element `place` handed to the `out` parameter
    /// `param_name` in device code, whose places have no address a call can
    /// be handed.
    fn refuse_out_path_in_device_code(&mut self, param_name: &str, place: &str, span: Span) {
        self.report_error_with_help(
            DiagnosticCode::TypFunctionSignature,
            format!(
                "'out' parameter '{param_name}' is handed '{place}' in device code, where a call takes no place"
            ),
            span,
            format!(
                "take the value as a plain parameter and return the new one, then store it: '{place} = …({place})'"
            ),
        );
    }

    /// Refuses the field or element `place` handed to the `out` parameter
    /// `param_name` of a runtime entry point or a compiler intrinsic, which
    /// takes only a whole variable: no address of a slot reaches it.
    fn refuse_out_path_to_builtin(&mut self, param_name: &str, place: &str, span: Span) {
        self.report_error_with_help(
            DiagnosticCode::TypFunctionSignature,
            format!(
                "'out' parameter '{param_name}' of a runtime function takes a whole variable, not '{place}': the runtime writes the variable it is handed and reaches no place inside one"
            ),
            span,
            copied_in_and_out_help(place),
        );
    }

    /// Refuses a second `out` argument taking `root`, which the call already
    /// took through `first`; `second` is the place this argument names.
    fn refuse_root_taken_twice(&mut self, root: &str, (first, second): (&str, &str), span: Span) {
        let (message, help) = if first == root && second == root {
            (
                format!("same variable passed twice as 'out': '{root}' appears more than once"),
                "pass a different 'var' to each 'out' parameter".to_string(),
            )
        } else {
            let copied = if second == root { first } else { second };
            (
                format!(
                    "same variable passed twice as 'out': '{first}' and '{second}' are both places in '{root}', and a call takes a variable once"
                ),
                format!(
                    "a call takes each variable once, whichever places in it the arguments name; copy '{copied}' into a 'var' ('var value = {copied}'), pass 'value', then store it back ('{copied} = value')"
                ),
            )
        };
        self.report_error_with_help(DiagnosticCode::TypOutParameterMisuse, message, span, help);
    }
}

/// How a call lends a value beside the places it takes.
#[derive(Clone, Copy)]
enum Lent {
    /// As an argument to a parameter that is not `out`.
    Argument,
    /// As the value a method call is made on.
    Receiver,
}

/// Whether the value `lent` names holds the place `taken`, a field or an
/// element: both start at one binding, and `lent` is a path toward `taken`,
/// field for field, any element standing for any other since two indexes
/// cannot be proved different.
///
/// A whole binding taken `out` beside itself lent (`f(x, x)`) is not
/// refused: the lent argument is a copy made before the call, which sees the
/// value the binding held before it.
fn holds_place(lent: &Expression, taken: &Expression) -> bool {
    let (lent_root, lent_links) = path_links(lent);
    let (taken_root, taken_links) = path_links(taken);
    let same_root = matches!(
        (&lent_root.node, &taken_root.node),
        (ExpressionKind::Identifier(a, _), ExpressionKind::Identifier(b, _)) if a == b
    );
    if !same_root {
        return false;
    }
    lent_links.len() < taken_links.len()
        && (lent_links.iter().zip(&taken_links)).all(|pair| match pair {
            (PathLink::Member(_, a, _), PathLink::Member(_, b, _)) => a.node == b.node,
            (PathLink::Index(..), PathLink::Index(..)) => true,
            (PathLink::Member(..), PathLink::Index(..))
            | (PathLink::Index(..), PathLink::Member(..)) => false,
        })
}

/// The refusal of `arg`, which is no binding and no path from one, handed to
/// the `out` parameter `param_name`: a call's result, or any other value.
fn not_a_place(param_name: &str, arg: &Expression) -> (DiagnosticCode, String, String) {
    let code = DiagnosticCode::TypFunctionSignature;
    let root = path_root(arg);
    let Some(call) = call_spelling(root) else {
        return (
            code,
            format!("'out' parameter '{param_name}' is handed a value, which no variable holds: {PLACE_RULE}"),
            "hold the value in a 'var' ('var value = …'), pass 'value', and read the new value from it after the call"
                .to_string(),
        );
    };
    let steps = steps_below(arg, None);
    let what = if steps.is_empty() {
        "the result of a call"
    } else {
        "a place in the result of a call"
    };
    (
        code,
        format!(
            "'out' parameter '{param_name}' is handed '{call}{steps}', {what}, which no variable holds: {PLACE_RULE}"
        ),
        format!("bind the result to a 'var' first ('var value = {call}'), then pass 'value{steps}'"),
    )
}

/// The refusal of `place`, a path into the gpu-resident `name`, handed on
/// the host to the `out` parameter `param_name`.
fn device_resident_refusal(
    param_name: &str,
    (place, rest): (&str, &str),
    name: &str,
) -> (DiagnosticCode, String, String) {
    (
        DiagnosticCode::TypFunctionSignature,
        format!(
            "'out' parameter '{param_name}' is handed '{place}', a place in the gpu-resident '{name}', which a call on the host cannot take"
        ),
        format!(
            "copy '{name}' to the host ('var host = {name}'), pass 'host{rest}', then copy it back ('{name} = host')"
        ),
    )
}

/// The refusal of `place`, a path through a slot of the tuple `tuple`,
/// handed to the `out` parameter `param_name`.
fn tuple_slot_refusal(
    param_name: &str,
    place: &str,
    tuple: &Expression,
) -> (DiagnosticCode, String, String) {
    let tuple = spelled_argument(tuple);
    (
        DiagnosticCode::TypFunctionSignature,
        format!(
            "'out' parameter '{param_name}' is handed '{place}', a slot of the tuple '{tuple}', which is not a place: a tuple's elements are fixed when it is built"
        ),
        format!(
            "copy it into a 'var' and pass that: 'var value = {place}', pass 'value', then build '{tuple}' again with 'value' in place of '{place}'"
        ),
    )
}

/// The refusal of `place`, a path into the map entry `entry` and on through
/// `rest`, handed to the `out` parameter `param_name`.
fn entry_refusal(
    param_name: &str,
    (place, rest): (&str, &str),
    entry: &Expression,
) -> (DiagnosticCode, String, String) {
    let spelled_entry = spelled_argument(entry);
    let map = if let ExpressionKind::Index(map, _) = &entry.node {
        spelled_argument(map)
    } else {
        "value".to_string()
    };
    let what = if rest.is_empty() {
        format!("an entry of the map '{map}'")
    } else {
        format!("a place in an entry of the map '{map}'")
    };
    (
        DiagnosticCode::TypFunctionSignature,
        format!(
            "'out' parameter '{param_name}' is handed '{place}', {what}, which is not a place: a map reaches an entry only through its key, so a call cannot write the entry in place"
        ),
        format!(
            "copy the entry into a 'var', pass that and store it back under its key: 'var value = {spelled_entry}', pass 'value{rest}', then '{spelled_entry} = value'"
        ),
    )
}

/// The refusal of `place`, reached from `name`, a binding of the kind
/// `binding` nothing may write through.
fn immutable_root_message(
    param_name: &str,
    place: &str,
    name: &str,
    binding: ImmutableBinding,
) -> String {
    let what = match binding {
        ImmutableBinding::Let => "it is declared with 'let'",
        ImmutableBinding::LentParameter => "it is a parameter its caller lends",
        ImmutableBinding::Constant => "it is a constant",
        ImmutableBinding::LoopBinding => "it is the binding of a 'for' loop",
    };
    format!(
        "'out' parameter '{param_name}' is handed '{place}', but '{name}' is immutable: {what}, and {PLACE_RULE}"
    )
}

/// The repair for `place`, reached from `name`, a binding of the kind
/// `binding` nothing may write through.
fn immutable_root_help(
    name: &str,
    binding: ImmutableBinding,
    (place, rest): (&str, &str),
    context: &Context,
) -> String {
    match binding {
        ImmutableBinding::Let => {
            format!("declare '{name}' with 'var' ('var {name} = …'), then pass '{place}'")
        }
        ImmutableBinding::LentParameter => {
            let declared = (context.resolve_info(name))
                .map(|info| format!(" {}", spelled(&info.ty)))
                .unwrap_or_default();
            format!(
                "declare the parameter 'out' ('{name} out{declared}') so the call writes its caller's value, or copy it into a 'var' ('var copy = {name}'), pass 'copy{rest}' and return the copy"
            )
        }
        ImmutableBinding::Constant => {
            format!("copy it into a 'var' ('var value = {name}') and pass 'value{rest}'")
        }
        ImmutableBinding::LoopBinding => format!(
            "iterate by index and pass the place through the collection: 'for i in 0..xs.length()', then pass 'xs[i]{rest}'"
        ),
    }
}

/// The repair that copies `place` into a `var`, passes the `var` and stores
/// it back.
fn copied_in_and_out_help(place: &str) -> String {
    format!(
        "copy it into a 'var', pass that and store it back: 'var value = {place}', pass 'value', then '{place} = value'"
    )
}

/// `arg` as the program writes it: a binding or a path from one, or a path
/// from a call's result; `value` for anything else.
fn spelled_argument(arg: &Expression) -> String {
    if let Some(spelled) = place_spelling(arg) {
        return spelled;
    }
    let root = path_root(arg);
    let root = if let ExpressionKind::Identifier(name, _) = &root.node {
        Some(name.clone())
    } else {
        call_spelling(root)
    };
    match root {
        Some(root) => format!("{root}{}", steps_below(arg, None)),
        None => "value".to_string(),
    }
}

/// A call as a refusal names it: `make()`, or `make(…)` when it is handed
/// arguments. `None` when `expr` is no call.
fn call_spelling(expr: &Expression) -> Option<String> {
    let ExpressionKind::Call(callee, args) = &expr.node else {
        return None;
    };
    let callee = place_spelling(callee).unwrap_or_else(|| "f".to_string());
    let args = if args.is_empty() { "" } else { "…" };
    Some(format!("{callee}({args})"))
}

/// The field and element steps of the place `arg` below `below`, one of the
/// places along it, or below its root when `below` is `None`, as the program
/// writes them: `[0].n` for `make()[0].n`, `.n` for `cells[k].n` below
/// `cells[k]`.
fn steps_below(arg: &Expression, below: Option<&Expression>) -> String {
    let (_, links) = path_links(arg);
    let skipped = below.map_or(0, |below| {
        (links.iter())
            .position(|link| std::ptr::eq(link.place(), below))
            .map_or(0, |at| at + 1)
    });
    (links.iter().skip(skipped))
        .map(|link| match link {
            PathLink::Member(_, field, _) => format!(".{}", field.node),
            PathLink::Index(_, index, _) => format!("[{}]", index_spelling(index)),
        })
        .collect()
}
