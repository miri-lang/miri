// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a `for` loop walks a value: by position, or through its iterator.
//!
//! [`TypeChecker::iteration_plan`] is the one answer, read by the checker to
//! type a loop and by lowering to build it. It is keyed on the traits the
//! language knows — `Indexed` and `Iterable` — and on the built-in sequences
//! the language indexes itself, never on the name of a library collection.
//!
//! A value that implements `Indexed` is walked by position, from index 0
//! while the index is below its `length()`. Any other `Iterable` is walked
//! through the iterator its `iterator()` hands out, one `next()` per pass.
//!
//! An iterator is declared as the trait `Iterator<T>`, and the class standing
//! behind it is known only from the bodies of `iterator()`: the checker notes,
//! for each `iterator()` declared to return a trait, the one class every value
//! it returns is an instance of. Those bodies are checked before any other
//! body of their module, so a loop anywhere in the module reads a complete
//! note. The loop calls `iterator()` as a written call is called, reaching a
//! subclass's override; the noted class is proof of the iterator the loop
//! holds only when no subclass overrides `iterator()` to return another, which
//! [`TypeChecker::settle_iteration_plans`] decides once every class of the
//! program is known. A second loop variable reads `Entries.value()`, which only
//! such a proven class can be shown to have.
//!
//! The settled plan of each loop is recorded against the loop's statement, and
//! lowering reads it from there rather than asking again.

use std::collections::HashMap;

use super::context::{
    class_ancestry, class_implements_trait, class_method_declaration, Context, TypeDefinition,
};
use super::TypeChecker;
use crate::ast::factory::make_type;
use crate::ast::statement::VariableDeclaration;
use crate::ast::type_arguments::type_arguments;
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeKind, ENTRIES_TRAIT_NAME, INDEXED_TRAIT_NAME,
    ITERABLE_TRAIT_NAME, ITERATOR_METHOD_NAME, ITERATOR_TRAIT_NAME, NEXT_METHOD_NAME,
    TUPLE_TYPE_NAME,
};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;

/// How a `for` loop over values of one type walks them.
#[derive(Debug, Clone, PartialEq)]
pub enum IterationPlan {
    /// Read by position, from index 0 while the index is below the length.
    Indexed(PositionalAccess),
    /// Through the iterator the value's `iterator()` hands out.
    Protocol(ProtocolWalk),
}

/// How a sequence walked by position is read.
#[derive(Debug, Clone, PartialEq)]
pub enum PositionalAccess {
    /// A list, an array or a tuple, which the language indexes itself: the
    /// loop reads its length and each element in place.
    Language,
    /// A class implementing `Indexed`, whose `length` and `element_at` the
    /// loop calls.
    Methods { class: String },
}

/// A walk through the iterator a value's `iterator()` hands out.
#[derive(Debug, Clone, PartialEq)]
pub struct ProtocolWalk {
    /// The class whose `iterator()` the loop calls.
    pub class: String,
    /// The iterator as the loop holds it: the class the `iterator()` body
    /// returns when the checker knows it, else the trait it is declared to
    /// return.
    pub iterator: Type,
    /// Whether [`Self::iterator`] is that class, so the loop calls its `next`
    /// and `value` directly rather than through the trait.
    pub iterator_is_class: bool,
    /// The type of `Entries.value()` when the iterator implements `Entries`,
    /// which a second loop variable reads.
    pub entries: Option<Type>,
}

/// The class instances a method declared to return a trait hands back.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum ReturnedClass {
    /// Every value it returns is an instance of this class, written in the
    /// owner's own generic parameters.
    Only(Type),
    /// It returns values of more than one type, or one that is not a class.
    Several,
}

/// The method whose returned values are being noted: its owner, its name and
/// the depth of the return-type stack its own `return`s are checked at.
#[derive(Debug, Clone)]
pub(crate) struct ReturningMethod {
    pub owner: String,
    pub method: String,
    pub depth: usize,
}

/// A loop the checker planned to walk through an iterator, kept until every
/// class of the program is known so its plan can be settled.
#[derive(Debug, Clone)]
pub(crate) struct ProtocolLoop {
    /// The `for` statement.
    pub statement: usize,
    /// The type of the value the loop walks.
    pub iterable: Type,
    /// The loop's second variable, when it names one.
    pub second: Option<VariableDeclaration>,
    /// Where the loop's iterable is written.
    pub span: Span,
}

impl TypeChecker {
    /// Records `plan` as how the `for` statement `statement` walks its value.
    /// A walk through an iterator is also kept for
    /// [`Self::settle_iteration_plans`].
    pub(crate) fn record_iteration_plan(
        &mut self,
        plan: &IterationPlan,
        statement: usize,
        (iterable, span): (&Type, Span),
        second: Option<&VariableDeclaration>,
    ) {
        if let IterationPlan::Protocol(_) = plan {
            self.protocol_loops.push(ProtocolLoop {
                statement,
                iterable: iterable.clone(),
                second: second.cloned(),
                span,
            });
        }
        self.iteration_plans.insert(statement, plan.clone());
    }

    /// The plan the checker settled for the `for` statement `statement`.
    pub(crate) fn recorded_iteration_plan(&self, statement: usize) -> Option<&IterationPlan> {
        self.iteration_plans.get(&statement)
    }

    /// Settles every walk through an iterator once every class of the program
    /// is known: a noted iterator class stops being proof when a subclass of
    /// the walked class overrides `iterator()` to return something else, and
    /// the loop then holds the iterator as the `Iterator` trait, whose `next`
    /// it now calls through the trait. A loop whose second variable read that
    /// class's `Entries` value is refused.
    pub(crate) fn settle_iteration_plans(&mut self) {
        for walked in std::mem::take(&mut self.protocol_loops) {
            let Some(IterationPlan::Protocol(walk)) = self.iteration_plans.get(&walked.statement)
            else {
                continue;
            };
            if !walk.iterator_is_class || !self.iterator_overridden_below(&walk.class) {
                continue;
            }
            let Some(unproven) = self.protocol_walk_through_trait(&walk.class, &walked.iterable)
            else {
                continue;
            };
            if let Some(second) = &walked.second {
                self.refuse_second_loop_variable(second, &walked.iterable, &unproven.iterator);
            }
            if let TypeKind::Custom(_, arguments) = &unproven.iterator.kind {
                let arguments = arguments.clone();
                let (trait_name, method) = (ITERATOR_TRAIT_NAME, NEXT_METHOD_NAME);
                self.record_trait_method_call(
                    trait_name,
                    method,
                    arguments.as_deref(),
                    walked.span,
                );
            }
            self.iteration_plans
                .insert(walked.statement, IterationPlan::Protocol(unproven));
        }
    }

    /// Whether some subclass of `class` overrides `iterator()` with a body
    /// not noted to return the class `class`'s own `iterator()` returns.
    fn iterator_overridden_below(&self, class: &str) -> bool {
        let definitions = &self.type_table.global_type_definitions;
        let noted = |owner: &str| {
            let key = (owner.to_string(), ITERATOR_METHOD_NAME.to_string());
            match self.returned_classes.get(&key) {
                Some(ReturnedClass::Only(ty)) => Some(&ty.kind),
                Some(ReturnedClass::Several) | None => None,
            }
        };
        let Some((owner, _)) = class_method_declaration(class, ITERATOR_METHOD_NAME, definitions)
        else {
            return false;
        };
        let expected = noted(owner);
        definitions.iter().any(|(name, definition)| {
            let TypeDefinition::Class(subclass) = definition else {
                return false;
            };
            name != class
                && subclass.methods.contains_key(ITERATOR_METHOD_NAME)
                && class_ancestry(name, definitions).any(|(ancestor, _)| ancestor == class)
                && (expected.is_none() || noted(name) != expected)
        })
    }

    /// How a `for` loop walks a value of type `iterable`, or `None` when the
    /// type is neither a built-in sequence nor a class reaching `Iterable`.
    pub(crate) fn iteration_plan(&self, iterable: &Type) -> Option<IterationPlan> {
        if let TypeKind::Tuple(_) = &iterable.kind {
            return Some(IterationPlan::Indexed(PositionalAccess::Language));
        }
        let class = super::named_class(&iterable.kind)?;
        if is_language_indexed(class) {
            return Some(IterationPlan::Indexed(PositionalAccess::Language));
        }
        let definitions = &self.type_table.global_type_definitions;
        let Some(TypeDefinition::Class(_)) = definitions.get(class) else {
            return None;
        };
        if class_implements_trait(class, INDEXED_TRAIT_NAME, definitions) {
            let class = class.to_string();
            return Some(IterationPlan::Indexed(PositionalAccess::Methods { class }));
        }
        if !class_implements_trait(class, ITERABLE_TRAIT_NAME, definitions) {
            return None;
        }
        self.protocol_walk(class, iterable)
            .map(IterationPlan::Protocol)
    }

    /// The walk through the iterator `class`'s `iterator()` hands out, for a
    /// value of type `iterable`.
    fn protocol_walk(&self, class: &str, iterable: &Type) -> Option<ProtocolWalk> {
        let Some(concrete) = self.returned_iterator_class(class, iterable) else {
            return self.protocol_walk_through_trait(class, iterable);
        };
        Some(ProtocolWalk {
            class: class.to_string(),
            iterator_is_class: true,
            entries: self.entries_value(&concrete),
            iterator: concrete,
        })
    }

    /// The walk through `class`'s iterator held as the `Iterator` trait at
    /// `iterable`'s element type, when the class behind it is not known.
    fn protocol_walk_through_trait(&self, class: &str, iterable: &Type) -> Option<ProtocolWalk> {
        let element = self.iterable_element_type(iterable)?;
        Some(ProtocolWalk {
            class: class.to_string(),
            iterator: make_type(TypeKind::Custom(
                ITERATOR_TRAIT_NAME.to_string(),
                Some(type_arguments([element])),
            )),
            iterator_is_class: false,
            entries: None,
        })
    }

    /// The class the `iterator()` body a loop over `iterable` calls returns,
    /// at `iterable`'s arguments, when every value that body returns is an
    /// instance of one class.
    fn returned_iterator_class(&self, class: &str, iterable: &Type) -> Option<Type> {
        let definitions = &self.type_table.global_type_definitions;
        let (owner, _) = class_method_declaration(class, ITERATOR_METHOD_NAME, definitions)?;
        let key = (owner.to_string(), ITERATOR_METHOD_NAME.to_string());
        let ReturnedClass::Only(returned) = self.returned_classes.get(&key)? else {
            return None;
        };
        let substitution =
            self.ancestor_substitution(class, &self.argument_types(iterable), owner)?;
        Some(self.substitute_type(returned, &substitution))
    }

    /// The type of `Entries.value()` on an iterator of class type `iterator`,
    /// when the class implements `Entries`.
    fn entries_value(&self, iterator: &Type) -> Option<Type> {
        let TypeKind::Custom(class, _) = &iterator.kind else {
            return None;
        };
        let arguments = self.argument_types(iterator);
        self.trait_argument_of_instantiation(class, &arguments, ENTRIES_TRAIT_NAME)
    }

    /// The type arguments `ty` is written with, each resolved to a type.
    pub(crate) fn argument_types(&self, ty: &Type) -> Vec<Type> {
        let TypeKind::Custom(_, Some(arguments)) = &ty.kind else {
            return Vec::new();
        };
        arguments
            .iter()
            .map(|argument| {
                self.extract_type_from_argument(argument)
                    .unwrap_or_else(|_| make_type(TypeKind::Error))
            })
            .collect()
    }

    /// Notes the type of a value the method being checked returns, when the
    /// method is declared to return a trait and the `return` is its own rather
    /// than a lambda's inside it.
    pub(crate) fn note_returned_value(&mut self, value: &Type, context: &Context) {
        let Some(frame) = &self.returning_method else {
            return;
        };
        if context.return_types.len() != frame.depth {
            return;
        }
        let declared_trait = context.return_types.last().is_some_and(|declared| {
            matches!(&declared.kind, TypeKind::Custom(name, _)
                if matches!(self.type_table.global_type_definitions.get(name), Some(TypeDefinition::Trait(_))))
        });
        if !declared_trait {
            return;
        }
        let key = (frame.owner.clone(), frame.method.clone());
        let is_class = matches!(&value.kind, TypeKind::Custom(name, _)
            if matches!(self.type_table.global_type_definitions.get(name), Some(TypeDefinition::Class(_))));
        let noted = match self.returned_classes.get(&key) {
            None if is_class => ReturnedClass::Only(value.clone()),
            Some(ReturnedClass::Only(earlier)) if is_class && earlier.kind == value.kind => return,
            None | Some(ReturnedClass::Only(_) | ReturnedClass::Several) => ReturnedClass::Several,
        };
        self.returned_classes.insert(key, noted);
    }

    /// Notes the value a block body ends on, which the method returns.
    pub(crate) fn note_tail_value(&mut self, tail: &crate::ast::Statement, context: &Context) {
        let crate::ast::StatementKind::Expression(expr) = &tail.node else {
            return;
        };
        if let Some(value) = self.type_table.types.get(&expr.id).cloned() {
            self.note_returned_value(&value, context);
        }
    }

    /// Refuses a loop walking an iterator in device code: a kernel has no heap
    /// to hold the iterator in, so only a sequence read by position can be
    /// walked there.
    pub(crate) fn refuse_device_protocol_loop(
        &mut self,
        iterable_type: &Type,
        context: &Context,
        span: Span,
    ) {
        if !context.in_gpu_function && context.gpu_for_depth == 0 {
            return;
        }
        self.report_error_with_help(
            DiagnosticCode::TarGpuIncompatibleSignature,
            format!(
                "a loop over '{iterable_type}' walks its iterator, which device code cannot run: \
                 a kernel has no heap to hold the iterator in"
            ),
            span,
            "loop over an array or a list, which device code reads by position, or walk the \
             collection on the host and pass the result in"
                .to_string(),
        );
    }
}

/// Whether the language indexes values of the class `name` itself: a list,
/// an array or a tuple, whose length and elements a loop reads in place.
fn is_language_indexed(name: &str) -> bool {
    matches!(
        BuiltinCollectionKind::from_name(name),
        Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array)
    ) || name == TUPLE_TYPE_NAME
}

/// The noted classes, keyed by the owner and name of the method returning
/// them.
pub(crate) type ReturnedClasses = HashMap<(String, String), ReturnedClass>;

/// The plan of each `for` statement over a value walked by position or
/// through an iterator, keyed by the statement.
pub(crate) type IterationPlans = HashMap<usize, IterationPlan>;
