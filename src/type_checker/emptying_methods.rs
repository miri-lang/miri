// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The methods that take a value out of their receiver and hand it back, as
//! their declarations state with `@empties`.
//!
//! A refusal of handing a value out of a place names the way to take it out
//! instead: the method of the optional that empties it, or the method of a
//! collection that takes an element out by its index. The compiler learns
//! which method that is from the mark alone, never from a method's name or
//! shape. A type with no marked method offers no way to take a value out, so
//! a refusal reading one of its elements has no repair to name.
//!
//! The mark holds on an instance method that writes its receiver, returns an
//! optional and takes no parameter but the `int` index the mark names, if it
//! names one. Anywhere else it is refused at the attribute, and a mark that
//! does not hold states nothing.

use crate::ast::expression::Expression;
use crate::ast::statement::{FunctionDeclarationData, Statement, StatementKind};
use crate::ast::types::TypeKind;
use crate::ast::Attribute;
use crate::ast::EMPTIES_ATTRIBUTE;
use crate::diagnostics::DiagnosticCode;

use super::body_writes::ReceiverWrites;
use super::context::MethodInfo;
use super::TypeChecker;

/// What a method marked `@empties` takes out of its receiver.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Emptied<'m> {
    /// The whole receiver, which it leaves empty.
    Receiver,
    /// The element at the index its parameter `index` carries.
    Element { index: &'m str },
}

/// What the method `method` of the type `owner`, declared as `info`, takes
/// out of its receiver as its mark states; `None` when it carries no mark,
/// or one that does not hold.
pub(crate) fn emptied_by<'m>(
    (owner, method): (&str, &str),
    info: &'m MethodInfo,
    writes: &ReceiverWrites,
) -> Option<Emptied<'m>> {
    let stated = stated_emptying(info)?;
    let holds = emptying_fault(Some((owner, method, info)), stated, writes).is_none();
    holds.then_some(stated)
}

/// What the mark on `info` states, read off the attribute alone.
fn stated_emptying(info: &MethodInfo) -> Option<Emptied<'_>> {
    (info.attributes.iter())
        .find(|attribute| attribute.name == EMPTIES_ATTRIBUTE)
        .map(stated_by)
}

/// What the `@empties` mark `attribute` states.
fn stated_by(attribute: &Attribute) -> Emptied<'_> {
    match attribute.argument.as_deref() {
        None => Emptied::Receiver,
        Some(index) => Emptied::Element {
            index: index.trim(),
        },
    }
}

/// Why a declaration marked as `stated` does not do what its mark states:
/// `declared` is the type, name and declaration of the method it is, and
/// `None` for a free function.
fn emptying_fault(
    declared: Option<(&str, &str, &MethodInfo)>,
    stated: Emptied<'_>,
    writes: &ReceiverWrites,
) -> Option<String> {
    let instance_method = declared.filter(|(_, _, info)| !info.is_static && !info.is_constructor);
    let Some((owner, method, info)) = instance_method else {
        return Some("it is not an instance method".to_string());
    };
    if !matches!(info.return_type.kind, TypeKind::Option(_)) {
        return Some("it returns no optional".to_string());
    }
    if !writes.writes_receiver(owner, method) {
        return Some("it never writes its receiver".to_string());
    }
    let Emptied::Element { index } = stated else {
        let (name, _) = info.params.first()?;
        return Some(format!(
            "it takes a parameter `{name}`, and the attribute names no index"
        ));
    };
    let Some(position) = (info.params.iter()).position(|(name, _)| name == index) else {
        return Some(format!("the attribute names no parameter `{index}`"));
    };
    let is_an_int = (info.params.get(position))
        .is_some_and(|(_, ty)| ty.kind == TypeKind::Int && !info.is_param_out(position));
    if !is_an_int {
        return Some(format!("`{index}` is not an `int`"));
    }
    (info.params.len() > 1).then(|| format!("it takes a parameter besides `{index}`"))
}

/// The help every refusal of a mark carries.
const EMPTIES_HELP: &str = "mark an instance method that writes its receiver and hands back \
                            what it takes out as an optional: `@empties` when it takes the \
                            whole receiver, `@empties(\"index\")` when it takes the element at \
                            its one `int` parameter `index`";

impl TypeChecker {
    /// Refuses each `@empties` mark among the top-level declarations of
    /// `statements` that does not hold, at the attribute.
    pub(crate) fn report_misused_emptying_marks<'s>(
        &mut self,
        statements: impl Iterator<Item = &'s Statement>,
    ) {
        for statement in statements {
            if let StatementKind::FunctionDeclaration(declaration) = &statement.node {
                self.refuse_emptying_mark(None, declaration);
            } else if let StatementKind::Class(class) = &statement.node {
                self.refuse_emptying_marks_of(&class.name, &class.body);
            } else if let StatementKind::Enum(name, _, _, methods, _, _) = &statement.node {
                self.refuse_emptying_marks_of(name, methods);
            }
        }
    }

    /// Refuses each `@empties` mark among `members`, the methods of the type
    /// `name` names, that does not hold.
    fn refuse_emptying_marks_of(&mut self, name: &Expression, members: &[Statement]) {
        let Some(owner) = self.type_identity_named_by(name).map(str::to_string) else {
            return;
        };
        for member in members {
            if let StatementKind::FunctionDeclaration(declaration) = &member.node {
                self.refuse_emptying_mark(Some(&owner), declaration);
            }
        }
    }

    /// Refuses the `@empties` mark on `declaration`, a method of `owner` or a
    /// free function, when it does not hold.
    fn refuse_emptying_mark(&mut self, owner: Option<&str>, declaration: &FunctionDeclarationData) {
        let Some(attribute) =
            (declaration.attributes.iter()).find(|attribute| attribute.name == EMPTIES_ATTRIBUTE)
        else {
            return;
        };
        let method = declaration.name.as_str();
        let declared = owner.and_then(|owner| {
            let definition = self.type_table.global_type_definitions.get(owner)?;
            Some((owner, method, definition.methods()?.get(method)?))
        });
        let fault = emptying_fault(declared, stated_by(attribute), &self.receiver_writes);
        let Some(reason) = fault else {
            return;
        };
        let spelled = owner.map_or_else(|| method.to_string(), |owner| format!("{owner}.{method}"));
        self.report_error_with_help(
            DiagnosticCode::TypFunctionSignature,
            format!("'{spelled}' cannot be marked `@empties`: {reason}"),
            attribute.span,
            EMPTIES_HELP.to_string(),
        );
    }
}
