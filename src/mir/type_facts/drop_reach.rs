// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether releasing a value of a type may run a drop hook anywhere inside it.
//!
//! Releasing a value releases everything it holds: the elements of a
//! collection, the inside of an option or tuple, the fields of a struct or
//! class and the payload of an enum. A `fn drop(self)` hook on any of those
//! runs when the last reference to the outer value goes, so moving where that
//! release happens moves where the hook prints, writes or frees.
//!
//! The answer errs towards "may": a value whose dynamic type is not known from
//! its static one — a trait-typed value, a closure and its captures, a type
//! parameter, a class some subclass of which has a hook — and a type nested
//! past the depth the mangler names are all taken to reach one.
//!
//! The same walk answers whether a type's drop thunk can run again inside
//! itself: whether what the instance holds can lead back to the instance. Only
//! a thunk on such a cycle has to bound the stack its release takes.

use crate::ast::type_arguments::{type_arguments, TypeArgument};
use crate::ast::types::{Type, TypeKind};
use crate::mir::instantiation::member_type_at;
use crate::mir::instantiation::shape::constructor_parts;
use crate::mir::symbol::token::MAX_TOKEN_DEPTH;
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

use super::drop_instantiations::stored_member_types;

/// The program's type definitions, read for which releases may run a drop hook.
pub struct DropHookReach<'d> {
    definitions: &'d HashMap<String, TypeDefinition>,
    /// The classes that name each class as their direct base.
    subclasses: HashMap<&'d str, Vec<&'d str>>,
}

impl<'d> DropHookReach<'d> {
    pub fn new(definitions: &'d HashMap<String, TypeDefinition>) -> Self {
        let mut subclasses: HashMap<&str, Vec<&str>> = HashMap::new();
        for (name, definition) in definitions {
            if let TypeDefinition::Class(class) = definition {
                if let Some(base) = &class.base_class {
                    subclasses.entry(base.as_str()).or_default().push(name);
                }
            }
        }
        Self {
            definitions,
            subclasses,
        }
    }

    /// Whether releasing a `ty` value may run a drop hook, on the value itself
    /// or on anything it holds.
    pub fn may_run_drop_hook(&self, ty: &Type) -> bool {
        Walk {
            reach: self,
            visited: HashSet::new(),
            sought: Sought::DropHook,
        }
        .reaches(ty, 0)
    }

    /// Whether releasing the instance of `name` at `args` may release another
    /// value of that same instance from inside itself, directly or through
    /// other types: the only way a release's depth can follow data.
    ///
    /// The instance's own subclasses are not followed from the start, because
    /// its thunk releases its members, not a value of some subclass; a member
    /// typed at a class is followed through every class that may stand for it.
    pub(super) fn may_recurse(&self, name: &str, args: Option<&[Type]>) -> bool {
        let arg_exprs = args.map(|args| type_arguments(args.iter().cloned()));
        let ty = Type::new(
            TypeKind::Custom(name.to_string(), arg_exprs.clone()),
            Default::default(),
        );
        let mut walk = Walk {
            reach: self,
            visited: HashSet::new(),
            sought: Sought::Instance(instance_symbol(name, &ty)),
        };
        walk.members_reach(&ty, name, arg_exprs.as_deref(), 0)
    }
}

/// What one walk is looking for.
enum Sought {
    /// A type with a `fn drop(self)` hook.
    DropHook,
    /// The instance whose thunk is named by this symbol.
    Instance(Symbol),
}

/// One question to [`DropHookReach`], with the instances already followed so a
/// type holding itself is followed once.
struct Walk<'r, 'd> {
    reach: &'r DropHookReach<'d>,
    visited: HashSet<Symbol>,
    sought: Sought,
}

impl Walk<'_, '_> {
    fn reaches(&mut self, ty: &Type, depth: usize) -> bool {
        if depth > MAX_TOKEN_DEPTH {
            return true;
        }
        if let TypeKind::Custom(name, args) = &ty.kind {
            return self.named_reaches(ty, name, args.as_deref(), depth);
        }
        if matches!(ty.kind, TypeKind::Generic(..) | TypeKind::Function(_)) {
            return true;
        }
        self.any_part_reaches(ty, depth)
    }

    /// Whether any type `ty` is built from reaches a hook.
    fn any_part_reaches(&mut self, ty: &Type, depth: usize) -> bool {
        constructor_parts(ty)
            .1
            .iter()
            .any(|part| self.reaches(part, depth + 1))
    }

    /// [`Walk::reaches`] for the named type `name` at `args`: the instance
    /// itself, what it holds, and every subclass it may stand for.
    fn named_reaches(
        &mut self,
        ty: &Type,
        name: &str,
        args: Option<&[TypeArgument]>,
        depth: usize,
    ) -> bool {
        let definitions = self.reach.definitions;
        let Some(definition) = definitions.get(name) else {
            return true;
        };
        match definition {
            TypeDefinition::Trait(_) | TypeDefinition::Generic(_) => true,
            TypeDefinition::Alias(alias) => {
                let template = member_type_at(alias.generics.as_deref(), args, &alias.template);
                self.reaches(&template, depth + 1)
            }
            TypeDefinition::Struct(_) | TypeDefinition::Class(_) | TypeDefinition::Enum(_) => {
                let instance = instance_symbol(name, ty);
                if self.is_sought(name, &instance) {
                    return true;
                }
                if !self.visited.insert(instance) {
                    return false;
                }
                self.members_reach(ty, name, args, depth) || self.any_subclass_reaches(name, depth)
            }
        }
    }

    /// Whether the instance named `name`, identified by `instance`, is what
    /// this walk is looking for.
    fn is_sought(&self, name: &str, instance: &Symbol) -> bool {
        match &self.sought {
            Sought::DropHook => {
                crate::type_checker::utils::has_drop_hook(name, self.reach.definitions)
            }
            Sought::Instance(sought) => sought == instance,
        }
    }

    /// Whether what `ty`, an instance of `name` at `args`, holds reaches what
    /// is sought: its type arguments — which a stdlib container stores out of
    /// its fields' sight — and its fields or payloads.
    fn members_reach(
        &mut self,
        ty: &Type,
        name: &str,
        args: Option<&[TypeArgument]>,
        depth: usize,
    ) -> bool {
        self.any_part_reaches(ty, depth)
            || stored_member_types(self.reach.definitions, name, args)
                .iter()
                .any(|member| self.reaches(member, depth + 1))
    }

    /// Whether a value of some class extending `name` reaches what is sought.
    fn any_subclass_reaches(&mut self, name: &str, depth: usize) -> bool {
        let reach = self.reach;
        let Some(subclasses) = reach.subclasses.get(name) else {
            return false;
        };
        subclasses.iter().any(|subclass| {
            let ty = Type::new(
                TypeKind::Custom(subclass.to_string(), None),
                Default::default(),
            );
            self.reaches(&ty, depth + 1)
        })
    }
}

/// The identity of the instance `ty` of `name`, as its drop thunk is named.
fn instance_symbol(name: &str, ty: &Type) -> Symbol {
    Symbol::type_thunk(ThunkKind::Drop, name, &constructor_parts(ty).1)
}
