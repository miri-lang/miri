// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which methods of a generic class instance are compiled, and at which
//! instances one reached only implicitly is withheld.
//!
//! A method is compiled at an instance where a lowered body calls it by name,
//! or where only a vtable slot or a container's element thunk names it. The
//! second kind is compiled only where the checker's obligations hold at the
//! instance: a constructed instance carries a vtable, and a container its
//! elements' `compare` and `equals`, whether or not the program ever converts
//! the instance to a trait or asks the container to order or match it.

use crate::ast::types::{Type, TypeKind};
use crate::error::syntax::Span;
use crate::mir::symbol::{Symbol, SymbolTable};
use crate::type_checker::{CompiledInstance, TypeChecker};
use std::collections::{HashMap, HashSet};

/// How a lowered program reaches a method of a generic class instance.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MethodDemand {
    /// A lowered body calls it by name.
    Called,
    /// Only a vtable slot or a container's element thunk names it, which the
    /// program runs only through a conversion to a trait or a container
    /// operation the checker pins the method at.
    Implied,
}

/// One method a generic class or enum compiles under its own name, at one of
/// its instantiations.
#[derive(Debug, Clone)]
pub(crate) struct InstanceMethod {
    pub(crate) class_name: String,
    pub(crate) method_name: String,
    /// Where the method is declared.
    pub(crate) span: Span,
    /// What the instantiation pins the class's parameters to.
    pub(crate) class_subs: HashMap<String, Type>,
    /// The instantiation's arguments, in the order the class declares them.
    pub(crate) mangle_args: Vec<(String, Type)>,
    pub(crate) symbol: Symbol,
}

impl InstanceMethod {
    /// The method as the checker answers its obligations, at `receiver`.
    pub(crate) fn compiled_at(&self, receiver: Type) -> CompiledInstance {
        CompiledInstance {
            class_name: self.class_name.clone(),
            method: self.method_name.clone(),
            substitution: self.class_subs.clone(),
            receiver,
            span: self.span,
        }
    }
}

/// The symbol of `method_name` of the generic class `class_name` at the
/// instantiation `class_args` spells.
pub(crate) fn instantiated_method_symbol(
    class_name: &str,
    method_name: &str,
    class_args: &[(String, Type)],
) -> Symbol {
    let class_args = class_args.iter().map(|(_, ty)| ty);
    Symbol::method(class_name, class_args, method_name, &[])
}

/// Whether a method reached only implicitly is compiled at an instance.
///
/// The checker pins a method only where the program does, so a method only a
/// vtable slot or an element thunk names is compiled only where its
/// obligations hold at the instance. One that fails there is withheld: nothing
/// that runs reaches it, and its vtable slot and element thunks are filled
/// with the runtime trap for a method not checked at its instance (MER_RT_014)
/// in case something does.
#[derive(Default)]
pub(crate) struct ImpliedMethodVerdicts {
    verdicts: HashMap<Symbol, bool>,
}

impl ImpliedMethodVerdicts {
    /// Whether `method`'s obligations hold at its instance, answered once.
    pub(crate) fn accepts(&mut self, checker: &mut TypeChecker, method: &InstanceMethod) -> bool {
        if let Some(&accepted) = self.verdicts.get(&method.symbol) {
            return accepted;
        }
        let receiver = super::monomorphized_self_type(
            &method.class_name,
            checker,
            &method.class_subs,
            method.span,
        );
        let accepted = checker
            .compiled_instance_refusals(&method.compiled_at(receiver))
            .is_empty();
        self.verdicts.insert(method.symbol.clone(), accepted);
        accepted
    }

    /// Whether the copy of the trait default `method` that `class_name`, a
    /// class with no parameters of its own, compiles holds its obligations
    /// at what the class's clauses pin, answered once.
    pub(crate) fn accepts_class_copy(
        &mut self,
        checker: &mut TypeChecker,
        class_name: &str,
        method: &str,
        span: Span,
    ) -> bool {
        let symbol = Symbol::method(class_name, &[], method, &[]);
        if let Some(&accepted) = self.verdicts.get(&symbol) {
            return accepted;
        }
        let receiver = Type::new(TypeKind::Custom(class_name.to_string(), None), span);
        let accepted = checker
            .compiled_instance_refusals(&CompiledInstance {
                class_name: class_name.to_string(),
                method: method.to_string(),
                substitution: HashMap::new(),
                receiver,
                span,
            })
            .is_empty();
        self.verdicts.insert(symbol, accepted);
        accepted
    }

    /// Every refused method nothing compiled after all.
    pub(crate) fn withheld(&self, symbols: &SymbolTable) -> HashSet<Symbol> {
        self.verdicts
            .iter()
            .filter(|(symbol, accepted)| !**accepted && !symbols.is_claimed(symbol))
            .map(|(symbol, _)| symbol.clone())
            .collect()
    }
}
