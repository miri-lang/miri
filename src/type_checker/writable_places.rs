// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether an expression names a place a write may land in.
//!
//! A place is a binding, or a path of fields and elements taken from one:
//! `grid[i]`, `self.slot`, `h.conn`. Whether a write may land there is read
//! off the binding the path starts at and off each step along it: the root
//! is a `var`, an `out` parameter or a method's own receiver, every field on
//! the path is one its type lets be written, and every index reaches an
//! element slot of a list or an array.
//!
//! A value indexed by key holds its entries where only a lookup reaches
//! them, so an entry is never a place: a write into one is a store under its
//! key, which a callee handed the entry could not make.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::place_path::{path_links, PathLink};
use crate::ast::statement::BindingResidency;
use crate::ast::types::{is_range_type_name, BuiltinCollectionKind, TypeKind};
use crate::ast::MemberVisibility;
use crate::diagnostics::RepairRequest;
use crate::type_checker::context::{collect_class_fields_all, Context, SymbolInfo, TypeDefinition};
use crate::type_checker::TypeChecker;

/// A place an expression names: the binding it starts at and the field and
/// element steps taken from it, the step nearest the root first.
pub(crate) struct PlacePath<'e> {
    pub(crate) root: PlaceRoot<'e>,
    pub(crate) steps: Vec<PlaceStep>,
}

impl PlacePath<'_> {
    /// The name of the binding the place starts at, `self` for a method's
    /// receiver.
    pub(crate) fn root_name(&self) -> &str {
        match &self.root {
            PlaceRoot::Var(name) | PlaceRoot::OutParameter(name) => name,
            PlaceRoot::Receiver => "self",
        }
    }
}

/// The binding a writable place starts at.
pub(crate) enum PlaceRoot<'e> {
    /// A `var` the scope owns.
    Var(&'e str),
    /// An `out` parameter: the caller's binding itself.
    OutParameter(&'e str),
    /// A method's own receiver, which a method that writes it is handed as
    /// its caller's own.
    Receiver,
}

/// One step of a place, as the place's type reads it.
pub(crate) enum PlaceStep {
    /// A field of a struct or a class.
    Field,
    /// An element slot of a list or an array.
    Element,
}

/// Why an expression names no place a write may land in.
#[derive(Clone, Copy)]
pub(crate) enum Refusal<'e> {
    /// The expression is no binding and no path from one: a literal, a
    /// call's result, an operator's.
    NotAPlace,
    /// The path starts at a binding nothing may write through, of the kind
    /// the second names.
    ImmutableRoot(&'e str, ImmutableBinding),
    /// A field on the path is one its class declares with `let`.
    ImmutableField(&'e str),
    /// A step on the path is an entry of a value indexed by key: the
    /// expression naming that entry, `cells[k]` in `cells[k].n`.
    Entry(&'e Expression),
    /// The path starts at a binding held in device memory.
    DeviceResident(&'e str),
    /// A step on the path is a slot of a tuple, whose elements are fixed
    /// when it is built: the expression naming the tuple, `t` in `t.0`.
    TupleSlot(&'e Expression),
}

/// What kind of binding a place nothing may write through starts at.
#[derive(Clone, Copy)]
pub(crate) enum ImmutableBinding {
    /// A binding declared with `let`, or bound by a pattern.
    Let,
    /// A parameter the caller lends, declared without `out`.
    LentParameter,
    /// A constant.
    Constant,
    /// The binding a `for` loop gives each element.
    LoopBinding,
}

impl TypeChecker {
    /// The place `expr` names when a write may land in it, or why it may not.
    pub(crate) fn writable_place<'e>(
        &self,
        expr: &'e Expression,
        context: &Context,
    ) -> Result<PlacePath<'e>, Refusal<'e>> {
        let (root, links) = path_links(expr);
        let root = place_root(root, !links.is_empty(), context)?;
        let steps = (links.iter())
            .map(|link| self.place_step(link, context))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(PlacePath { root, steps })
    }

    /// The step `link` takes, when a write may land through it.
    fn place_step<'e>(
        &self,
        link: &PathLink<'e>,
        context: &Context,
    ) -> Result<PlaceStep, Refusal<'e>> {
        match link {
            PathLink::Member(holder, prop, _) => {
                if self
                    .get_type(holder.id)
                    .is_some_and(|ty| ty.kind.is_tuple())
                {
                    return Err(Refusal::TupleSlot(holder));
                }
                let ExpressionKind::Identifier(field, _) = &prop.node else {
                    return Err(Refusal::NotAPlace);
                };
                self.field_step(holder, field, context)
            }
            PathLink::Index(collection, index, entry) => {
                self.element_step(collection, index, entry)
            }
        }
    }

    /// The step to field `field` of the value `holder` is, when its type
    /// declares that field and lets it be written.
    fn field_step<'e>(
        &self,
        holder: &Expression,
        field: &'e str,
        context: &Context,
    ) -> Result<PlaceStep, Refusal<'e>> {
        let Some(TypeKind::Custom(name, _)) = self.get_type(holder.id).map(|ty| &ty.kind) else {
            return Err(Refusal::NotAPlace);
        };
        let definitions = &self.type_table.global_type_definitions;
        let definition = (context.resolve_type_definition(name)).or_else(|| definitions.get(name));
        let is_mutable = match definition {
            Some(TypeDefinition::Struct(definition)) => (definition.fields.iter())
                .any(|(declared, _, _)| declared == field)
                .then_some(true),
            Some(TypeDefinition::Class(definition)) => {
                collect_class_fields_all(definition, definitions)
                    .into_iter()
                    .find_map(|(declared, info)| (declared == field).then_some(info.mutable))
            }
            Some(
                TypeDefinition::Enum(_)
                | TypeDefinition::Generic(_)
                | TypeDefinition::Alias(_)
                | TypeDefinition::Trait(_),
            )
            | None => None,
        };
        match is_mutable {
            Some(true) => Ok(PlaceStep::Field),
            Some(false) => Err(Refusal::ImmutableField(field)),
            None => Err(Refusal::NotAPlace),
        }
    }

    /// The step to the element of `collection` at `index`, when that is an
    /// element slot: an index into a list or an array, never a range slicing
    /// one, and never a key into a value indexed by key: `indexed` is the
    /// whole `collection[index]`.
    fn element_step<'e>(
        &self,
        collection: &Expression,
        index: &Expression,
        indexed: &'e Expression,
    ) -> Result<PlaceStep, Refusal<'e>> {
        let Some(collection_ty) = self.get_type(collection.id) else {
            return Err(Refusal::NotAPlace);
        };
        if crate::type_checker::expressions::access::is_indexed_by_key(collection_ty) {
            return Err(Refusal::Entry(indexed));
        }
        let is_range = matches!(
            self.get_type(index.id).map(|ty| &ty.kind),
            Some(TypeKind::Custom(name, _)) if is_range_type_name(name)
        );
        match collection_ty.kind.as_builtin_collection() {
            Some(BuiltinCollectionKind::List | BuiltinCollectionKind::Array) if !is_range => {
                Ok(PlaceStep::Element)
            }
            Some(
                BuiltinCollectionKind::List
                | BuiltinCollectionKind::Array
                | BuiltinCollectionKind::Map
                | BuiltinCollectionKind::Set,
            )
            | None => Err(Refusal::NotAPlace),
        }
    }
}

/// The binding the place starting at `root` starts at, when a write may land
/// through it. A binding held in device memory is a place only whole:
/// `is_path` says a field or an element of it is reached.
fn place_root<'e>(
    root: &'e Expression,
    is_path: bool,
    context: &Context,
) -> Result<PlaceRoot<'e>, Refusal<'e>> {
    let ExpressionKind::Identifier(name, _) = &root.node else {
        return Err(Refusal::NotAPlace);
    };
    if name == "self" {
        return Ok(PlaceRoot::Receiver);
    }
    let Some(info) = context.resolve_info(name) else {
        return Err(Refusal::NotAPlace);
    };
    if is_path && matches!(info.residency, BindingResidency::Gpu) {
        return Err(Refusal::DeviceResident(name));
    }
    match (info.mutable && !info.is_constant, info.is_parameter) {
        (true, true) => Ok(PlaceRoot::OutParameter(name)),
        (true, false) => Ok(PlaceRoot::Var(name)),
        (false, _) => Err(Refusal::ImmutableRoot(name, immutable_binding(info))),
    }
}

/// What kind of binding `info`, which nothing may write through, describes.
fn immutable_binding(info: &SymbolInfo) -> ImmutableBinding {
    if info.is_constant {
        ImmutableBinding::Constant
    } else if info.is_parameter {
        ImmutableBinding::LentParameter
    } else if info.is_loop_binding {
        ImmutableBinding::LoopBinding
    } else {
        ImmutableBinding::Let
    }
}

/// The `let` keyword of the binding `name` resolves to, rewritten as `var`.
///
/// A constant is not repaired into a `var`: it is a different declaration
/// form, so rewriting the keyword would not compile. A binding with no
/// recorded keyword — a parameter, or a statement binding several names —
/// has none to rewrite.
pub(crate) fn let_to_var_repair(name: &str, context: &Context) -> Option<RepairRequest> {
    if context.is_constant(name) {
        return None;
    }
    let info = context.resolve_info(name)?;
    Some(RepairRequest::LetToVar {
        keyword_start: info.declaration_keyword_start?,
        module_scope: info.module_scope,
        is_public: matches!(info.visibility, MemberVisibility::Public),
    })
}
