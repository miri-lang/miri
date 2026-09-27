// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every instantiation of a generic type some value of the program is released
//! at, closed over the fields those instantiations store.
//!
//! A value is released through the drop thunk of the exact instantiation it
//! was built at, because only that thunk reads each field at the type the
//! instance stores. The method-body registry lists the instantiations whose
//! methods were lowered, which is not the same set: a value can be held — in a
//! local, a captured variable, a field or a payload — at an instantiation no
//! call reaches. So the set a drop thunk is emitted for is gathered from the
//! lowered bodies themselves, and every field of every instantiation found is
//! followed until nothing new appears.
//!
//! The method-body registry stays separate. An element-method thunk calls a
//! lowered body, and naming one for an instantiation no body was lowered at
//! would reference a symbol nothing defines.

use crate::ast::expression::Expression;
use crate::ast::factory::type_expr_non_null;
use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::{
    field_types, has_a_monomorphized_spelling, instantiation_argument, member_type_at,
};
use crate::mir::lowering::instantiation_limits::{constructor_parts, mentions_open_parameter};
use crate::mir::symbol::token::MAX_TOKEN_DEPTH;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::{AggregateKind, Body, Rvalue, StatementKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

/// Concrete type-argument tuples, keyed by the generic type they instantiate.
pub(crate) type Instantiations = HashMap<String, Vec<Vec<Type>>>;

/// The instantiations a drop thunk is emitted for: every tuple `registry`
/// records, every concrete instantiation a type `bodies` hold names, and every
/// one a field or payload of those stores, followed until closed.
///
/// Each tuple is kept once per thunk symbol, in the order it was first found,
/// so the thunks come out in the same order on every build.
pub(crate) fn drop_instantiations<'b>(
    definitions: &HashMap<String, TypeDefinition>,
    registry: &Instantiations,
    bodies: impl IntoIterator<Item = &'b Body>,
) -> Instantiations {
    let mut closure = Closure {
        definitions,
        found: HashMap::new(),
        seen: HashSet::new(),
        pending: Vec::new(),
    };
    let mut registered: Vec<&String> = registry.keys().collect();
    registered.sort_unstable();
    for name in registered {
        for args in registry.get(name).into_iter().flatten() {
            closure.admit(name, args.clone());
        }
    }
    let mut declared: Vec<&String> = definitions.keys().collect();
    declared.sort_unstable();
    for name in declared {
        for ty in declared_member_types(definitions, name) {
            closure.walk(&ty, 0);
        }
    }
    for body in bodies {
        for ty in held_types(body) {
            closure.walk(ty, 0);
        }
    }
    closure.close();
    closure.found
}

/// The instantiations found so far, and the ones whose fields are still to be
/// followed.
struct Closure<'d> {
    definitions: &'d HashMap<String, TypeDefinition>,
    found: Instantiations,
    seen: HashSet<Symbol>,
    pending: Vec<(String, Vec<Type>)>,
}

impl Closure<'_> {
    /// Record `name` at `args`, once per drop-thunk symbol.
    fn admit(&mut self, name: &str, args: Vec<Type>) {
        if !self
            .seen
            .insert(Symbol::type_thunk(ThunkKind::Drop, name, &args))
        {
            return;
        }
        self.found
            .entry(name.to_string())
            .or_default()
            .push(args.clone());
        self.pending.push((name.to_string(), args));
    }

    /// Record every concrete instantiation spelled anywhere inside `ty`.
    ///
    /// The descent stops where the symbol mangler stops naming a type, so a
    /// field that nests its own type deeper on every instantiation ends.
    fn walk(&mut self, ty: &Type, depth: usize) {
        if depth > MAX_TOKEN_DEPTH {
            return;
        }
        if let Some((name, args)) = self.concrete_instantiation(ty) {
            self.admit(&name, args);
        }
        for part in constructor_parts(ty).1 {
            self.walk(&part, depth + 1);
        }
    }

    /// `ty` as an instantiation of a generic struct, class or enum at arguments
    /// that are all concrete, or `None`.
    fn concrete_instantiation(&self, ty: &Type) -> Option<(String, Vec<Type>)> {
        let TypeKind::Custom(name, Some(arg_exprs)) = &ty.kind else {
            return None;
        };
        let arity = self.definitions.get(name)?.generics()?.len();
        let args: Vec<Type> = arg_exprs
            .iter()
            .map(instantiation_argument)
            .collect::<Option<_>>()?;
        let is_concrete = args.len() == arity
            && args.iter().all(|arg| {
                has_a_monomorphized_spelling(&arg.kind)
                    && !mentions_open_parameter(arg, self.definitions)
            });
        is_concrete.then(|| (name.clone(), args))
    }

    /// Follow the fields and payloads of every instantiation admitted, until a
    /// pass admits nothing new.
    fn close(&mut self) {
        while let Some((name, args)) = self.pending.pop() {
            let arg_exprs: Vec<Expression> = args.into_iter().map(type_expr_non_null).collect();
            for member in stored_member_types(self.definitions, &name, &arg_exprs) {
                self.walk(&member, 0);
            }
        }
    }
}

/// Every type `body` holds a value of: its locals, the variables its closures
/// capture, and the classes it constructs.
fn held_types(body: &Body) -> impl Iterator<Item = &Type> {
    let locals = body.local_decls.iter().map(|decl| &decl.ty);
    let captures = body.closure_capture_types.values().flatten();
    let constructed = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, Rvalue::Aggregate(AggregateKind::Class(ty), _))
            | StatementKind::Reassign(_, Rvalue::Aggregate(AggregateKind::Class(ty), _)) => {
                Some(ty)
            }
            StatementKind::Assign(..)
            | StatementKind::Reassign(..)
            | StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Nop
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_) => None,
        });
    locals.chain(captures).chain(constructed)
}

/// The type every field or payload of `name` is declared at — what its shared
/// drop thunk releases.
fn declared_member_types(definitions: &HashMap<String, TypeDefinition>, name: &str) -> Vec<Type> {
    match definitions.get(name) {
        Some(TypeDefinition::Enum(def)) => def.variants.values().flatten().cloned().collect(),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Class(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Trait(_),
        )
        | None => field_types(definitions, name, None).unwrap_or_default(),
    }
}

/// The type every field or payload of `name` stores at `args` — what that
/// instantiation's drop thunk releases.
fn stored_member_types(
    definitions: &HashMap<String, TypeDefinition>,
    name: &str,
    args: &[Expression],
) -> Vec<Type> {
    match definitions.get(name) {
        Some(TypeDefinition::Enum(def)) => def
            .variants
            .values()
            .flatten()
            .map(|declared| member_type_at(def.generics.as_deref(), Some(args), declared))
            .collect(),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Class(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_)
            | TypeDefinition::Trait(_),
        )
        | None => field_types(definitions, name, Some(args)).unwrap_or_default(),
    }
}
