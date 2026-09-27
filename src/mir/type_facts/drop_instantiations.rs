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
//! An instantiation inference left partly unbound (`Result<String, E>` from
//! `let r = Result.Ok("s")`) is collected too, at the arguments it carries: a
//! value built that way stores nothing at the unbound slot, so its drop
//! function releases the bound ones and skips that slot.
//!
//! A field that nests its own type deeper on every instantiation
//! (`Node<T>` holding a `Node<List<T>>`) would have the closure grow without
//! end; it is bounded by the depth any instance may nest to, the same bound
//! lowering refuses a growing instantiation at.
//!
//! The method-body registry stays separate. An element-method thunk calls a
//! lowered body, and naming one for an instantiation no body was lowered at
//! would reference a symbol nothing defines.

use super::DropInstantiationRefusal;
use crate::ast::expression::Expression;
use crate::ast::factory::type_expr_non_null;
use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::shape::{
    constructor_parts, instance_type_depth, is_partially_bound_instantiation,
    with_unbound_arguments_normalized, MAX_INSTANCE_TYPE_DEPTH,
};
use crate::mir::instantiation::{field_types, instantiation_argument, member_type_at};
use crate::mir::symbol::token::MAX_TOKEN_DEPTH;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::mir::{AggregateKind, Body, Rvalue, StatementKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::HashMap;

/// Concrete type-argument tuples, keyed by the generic type they instantiate.
pub(crate) type Instantiations = HashMap<String, Vec<Vec<Type>>>;

/// The instantiations a drop thunk is emitted for: every concrete tuple
/// `registry` records, every concrete instantiation a type `bodies` hold
/// names, and every one a field or payload of those stores, followed until
/// closed.
///
/// Each tuple is kept once per thunk symbol, in the order it was first found,
/// so the thunks come out in the same order on every build. Refused when an
/// instantiation nests past the depth any instance may nest to, or when two
/// different instantiations would share one thunk symbol.
pub(crate) fn drop_instantiations<'b>(
    definitions: &HashMap<String, TypeDefinition>,
    registry: &Instantiations,
    bodies: impl IntoIterator<Item = &'b Body>,
) -> Result<Instantiations, DropInstantiationRefusal> {
    let mut closure = Closure {
        definitions,
        found: HashMap::new(),
        seen: HashMap::new(),
        pending: Vec::new(),
    };
    let mut registered: Vec<&String> = registry.keys().collect();
    registered.sort_unstable();
    for name in registered {
        for args in registry.get(name).into_iter().flatten() {
            let args = with_unbound_arguments_normalized(definitions, name, args);
            if closure.is_concrete(name, &args) {
                closure.admit(name, args)?;
            }
        }
    }
    let mut declared: Vec<&String> = definitions.keys().collect();
    declared.sort_unstable();
    for name in declared {
        for ty in declared_member_types(definitions, name) {
            closure.walk(&ty, 0)?;
        }
    }
    for body in bodies {
        for ty in held_types(body) {
            closure.walk(ty, 0)?;
        }
    }
    closure.close()?;
    Ok(closure.found)
}

/// The instantiations found so far, and the ones whose fields are still to be
/// followed.
struct Closure<'d> {
    definitions: &'d HashMap<String, TypeDefinition>,
    found: Instantiations,
    /// The arguments first admitted under each thunk symbol.
    seen: HashMap<Symbol, Vec<TypeKind>>,
    pending: Vec<(String, Vec<Type>)>,
}

impl Closure<'_> {
    /// Record `name` at `args`, once per drop-thunk symbol.
    ///
    /// Refused past the depth bound, and when a different instantiation
    /// already claimed the same symbol because their arguments have no name:
    /// one thunk laid out for one of them would release the other at the
    /// wrong type.
    fn admit(&mut self, name: &str, args: Vec<Type>) -> Result<(), DropInstantiationRefusal> {
        let depth = instance_type_depth(&args);
        if depth > MAX_INSTANCE_TYPE_DEPTH {
            return Err(DropInstantiationRefusal::TooDeep {
                name: name.to_string(),
                args,
                depth,
            });
        }
        let symbol = Symbol::type_thunk(ThunkKind::Drop, name, &args);
        let kinds: Vec<TypeKind> = args.iter().map(|arg| arg.kind.clone()).collect();
        if let Some(first) = self.seen.get(&symbol) {
            if symbol.has_an_unnameable_argument() && *first != kinds {
                return Err(DropInstantiationRefusal::Unnameable {
                    name: name.to_string(),
                    symbol: symbol.written().to_string(),
                });
            }
            return Ok(());
        }
        self.seen.insert(symbol, kinds);
        self.found
            .entry(name.to_string())
            .or_default()
            .push(args.clone());
        self.pending.push((name.to_string(), args));
        Ok(())
    }

    /// Record every concrete instantiation spelled anywhere inside `ty`,
    /// reading an alias as the type it stands for, as the drop path does.
    ///
    /// The descent stops where the symbol mangler stops naming a type.
    fn walk(&mut self, ty: &Type, depth: usize) -> Result<(), DropInstantiationRefusal> {
        if depth > MAX_TOKEN_DEPTH {
            return Ok(());
        }
        if let Some((name, args)) = self.concrete_instantiation(ty) {
            self.admit(&name, args)?;
        }
        if let Some(template) = self.alias_template(ty) {
            self.walk(&template, depth + 1)?;
        }
        for part in constructor_parts(ty).1 {
            self.walk(&part, depth + 1)?;
        }
        Ok(())
    }

    /// The type the alias `ty` names stands for, or `None` when `ty` names no
    /// alias.
    fn alias_template(&self, ty: &Type) -> Option<Type> {
        let TypeKind::Custom(name, _) = &ty.kind else {
            return None;
        };
        let Some(TypeDefinition::Alias(alias)) = self.definitions.get(name) else {
            return None;
        };
        Some(alias.template.clone())
    }

    /// `ty` as an instantiation of a generic struct, class or enum a drop
    /// function can be named for, or `None`.
    fn concrete_instantiation(&self, ty: &Type) -> Option<(String, Vec<Type>)> {
        let TypeKind::Custom(name, Some(arg_exprs)) = &ty.kind else {
            return None;
        };
        let args: Vec<Type> = arg_exprs
            .iter()
            .map(instantiation_argument)
            .collect::<Option<_>>()?;
        let args = with_unbound_arguments_normalized(self.definitions, name, &args);
        self.is_concrete(name, &args).then(|| (name.clone(), args))
    }

    /// Whether `args` instantiate the generic type `name` at arguments a drop
    /// function can be named for: concrete, or left unbound by inference at
    /// the type's own parameter ([`is_partially_bound_instantiation`]).
    fn is_concrete(&self, name: &str, args: &[Type]) -> bool {
        is_partially_bound_instantiation(self.definitions, name, args)
    }

    /// Follow the fields and payloads of every instantiation admitted, until a
    /// pass admits nothing new.
    fn close(&mut self) -> Result<(), DropInstantiationRefusal> {
        while let Some((name, args)) = self.pending.pop() {
            let arg_exprs: Vec<Expression> = args.into_iter().map(type_expr_non_null).collect();
            for member in stored_member_types(self.definitions, &name, &arg_exprs) {
                self.walk(&member, 0)?;
            }
        }
        Ok(())
    }
}

/// Every type `body` holds a value of: its locals, the variables its closures
/// capture, and the structs and classes it constructs. An enum construction
/// carries no type arguments of its own; the local it is assigned to does.
fn held_types(body: &Body) -> impl Iterator<Item = &Type> {
    let locals = body.local_decls.iter().map(|decl| &decl.ty);
    let captures = body.closure_capture_types.values().flatten();
    let constructed = body
        .basic_blocks
        .iter()
        .flat_map(|block| &block.statements)
        .filter_map(|statement| match &statement.kind {
            StatementKind::Assign(_, rvalue) | StatementKind::Reassign(_, rvalue) => {
                constructed_type(rvalue)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::Nop
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_) => None,
        });
    locals.chain(captures).chain(constructed)
}

/// The struct or class type `rvalue` constructs, if it constructs one.
fn constructed_type(rvalue: &Rvalue) -> Option<&Type> {
    let Rvalue::Aggregate(kind, _) = rvalue else {
        return None;
    };
    match kind {
        AggregateKind::Struct(ty) | AggregateKind::Class(ty) => Some(ty),
        AggregateKind::Tuple
        | AggregateKind::Array
        | AggregateKind::List
        | AggregateKind::FormattedString
        | AggregateKind::Set
        | AggregateKind::Map
        | AggregateKind::Enum(_, _)
        | AggregateKind::Option
        | AggregateKind::Closure(_, _) => None,
    }
}

/// The type every field or payload of `name` is declared at — what its shared
/// drop thunk releases.
fn declared_member_types(definitions: &HashMap<String, TypeDefinition>, name: &str) -> Vec<Type> {
    match definitions.get(name) {
        Some(TypeDefinition::Enum(def)) => def.variants.values().flatten().cloned().collect(),
        Some(TypeDefinition::Alias(alias)) => vec![alias.template.clone()],
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Class(_)
            | TypeDefinition::Generic(_)
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
