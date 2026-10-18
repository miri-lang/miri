// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The declared type a MIR place reaches, and the type each slot a value
//! flows into declares, read from its body's declarations.
//!
//! A place names a local and a chain of projections. What it reaches is read
//! off the local's declared type one projection at a time — a field at the
//! instance's type arguments, an element of a collection, a closure capture —
//! so a question about it is asked of the type it actually holds, arguments
//! included, rather than of the root local's.
//!
//! The same reading answers what an aggregate's members, a collection call's
//! element and key arguments are declared at. This is the one slot-type
//! authority: every pass and check that asks where a value is going asks it.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::literal::Literal;
use crate::ast::type_identity;
use crate::ast::types::{BuiltinCollectionKind, Type, TypeKind, TUPLE_TYPE_NAME};
use crate::mir::lowering::{field_type_in_instance, type_argument};
use crate::mir::rvalue::AggregateKind;
use crate::mir::symbol::Symbol;
use crate::mir::terminator::TerminatorKind;
use crate::mir::types::MirType;
use crate::mir::{Body, Local, LocalDecl, Operand, Place, PlaceElem};
use crate::runtime_fns::ElementRole;
use crate::type_checker::context::TypeDefinition;
use crate::type_checker::instantiation_requirements::{spells_where, Visit};
use std::collections::HashMap;

/// The declarations a body keeps that say what type a place reaches.
pub struct PlaceTypes<'a> {
    pub local_decls: &'a [LocalDecl],
    pub field_types: &'a HashMap<String, Vec<Type>>,
    pub class_type_params: &'a HashMap<String, Vec<String>>,
    pub closure_capture_types: &'a HashMap<Local, Vec<Type>>,
    /// The program's type table, which says what an enum's variants and a
    /// struct's or class's fields hold where the body records no entry.
    pub definitions: &'a HashMap<String, TypeDefinition>,
    /// The type parameters the body names, its own and its class's.
    pub type_params: &'a std::collections::HashSet<String>,
    /// The parameters the body leaves open, as a shared generic body does.
    pub open_params: &'a std::collections::HashSet<String>,
    /// Whether a field whose type cannot be read at its instance is unread.
    /// Reference counting reads it as the body records it instead.
    pub strict: bool,
}

impl<'a> PlaceTypes<'a> {
    /// The place types of `body`.
    pub fn of(body: &'a Body) -> Self {
        Self {
            local_decls: &body.local_decls,
            field_types: &body.field_types,
            class_type_params: &body.class_type_params,
            closure_capture_types: &body.closure_capture_types,
            definitions: body.rc_classifier.definitions(),
            type_params: &body.type_params,
            open_params: &body.open_params,
            strict: true,
        }
    }

    /// The type `operand` reads: a place's declared type, a constant's or a
    /// function's own; `None` for the absent constant, whose type is the
    /// slot it meets, and where the place's type cannot be read.
    pub fn operand_type(&self, operand: &Operand) -> Option<Type> {
        match operand {
            Operand::Constant(constant) => {
                (!matches!(constant.literal, Literal::None)).then(|| constant.ty.clone())
            }
            Operand::Function(function) => Some(function.ty.clone()),
            Operand::Copy(place) | Operand::Move(place) => self.declared(place),
        }
    }

    /// The type member `slot` of an aggregate of `kind` built into `dest`
    /// declares, or `None` where it cannot be read.
    ///
    /// A struct or class member is its field at the instance's type
    /// arguments; an enum's slot 0 is its discriminant and each further one a
    /// payload field of the variant, at the enum's arguments; a closure's are
    /// its captures; a collection literal's are its elements, a map's keys
    /// and values alternating.
    pub fn aggregate_slot(&self, kind: &AggregateKind, dest: &Place, slot: usize) -> Option<Type> {
        let target = self.declared(dest)?;
        match kind {
            AggregateKind::Option(built) => optional_payload(built)
                .or_else(|| (slot == 0).then(|| optional_payload(&target)).flatten()),
            AggregateKind::Tuple(built) => {
                tuple_member(built, slot).or_else(|| tuple_member(&target, slot))
            }
            // The destination's declared type is the instance built: the
            // type the aggregate names was spelled where it was lowered, and
            // inside a generic body it may still read the body's parameters.
            AggregateKind::Struct(built) | AggregateKind::Class(built) => {
                let names_one_type = matches!(
                    (&target.kind, &built.kind),
                    (TypeKind::Custom(dest_name, _), TypeKind::Custom(built_name, _))
                        if dest_name == built_name
                );
                let instance = if names_one_type { &target } else { built };
                self.instance_field(instance, slot)
            }
            AggregateKind::Enum(enum_name, variant) => {
                if slot == 0 {
                    return Some(Type::new(TypeKind::Int, target.span));
                }
                self.variant_field(enum_name, variant, &target, slot - 1)
            }
            AggregateKind::Closure(_, _) => self
                .closure_capture_types
                .get(&dest.local)?
                .get(slot)
                .cloned(),
            AggregateKind::Array | AggregateKind::List | AggregateKind::Set => {
                indexed_element_type(&target)
            }
            AggregateKind::Map => {
                let role = if slot.is_multiple_of(2) {
                    ElementRole::Key
                } else {
                    ElementRole::Value
                };
                element_slot(&target, role)
            }
            AggregateKind::FormattedString => Some(Type::new(TypeKind::String, target.span)),
        }
    }

    /// Field `index` of the struct or class `instance`, at its arguments.
    ///
    /// An instance written with its arguments is read from the definition at
    /// those arguments, through the `extends` chain for an inherited field
    /// ([`crate::mir::instantiation::field_types`]): another instance of the
    /// body's own generic type — `Sub<V?>` built inside `Sub<V>` — is not
    /// the instantiation the body records. One written bare is this body's
    /// own, as it records it. `None` where the field's type still names a
    /// parameter nothing binds: no slot type is better than one substituted
    /// for a different instance.
    fn instance_field(&self, instance: &Type, index: usize) -> Option<Type> {
        let TypeKind::Custom(name, args) = &instance.kind else {
            return None;
        };
        let field = match args {
            Some(args) => {
                crate::mir::instantiation::field_types(self.definitions, name, Some(args))?
                    .get(index)?
                    .clone()
            }
            None => match self.field_types.get(name.as_str()) {
                Some(fields) => fields.get(index)?.clone(),
                None => crate::mir::instantiation::field_types(self.definitions, name, None)?
                    .get(index)?
                    .clone(),
            },
        };
        self.binds_every_name(&field).then_some(field)
    }

    /// Whether `kind` names a parameter this body leaves open.
    ///
    /// Such a type has no settled representation: what an open `T` holds,
    /// and so how its optional is laid out, is the instantiation's to say.
    /// Every rule comparing representations leaves it alone.
    pub fn names_open_parameter(&self, kind: &TypeKind) -> bool {
        names_open_parameter(kind, self.type_params)
    }

    /// Whether every name `ty` spells is bound: a type the program defines,
    /// a parameter this body leaves open, or a built-in collection.
    ///
    /// Two spellings count as bound beside those. An enum argument written as
    /// the enum's own parameter in that parameter's position —
    /// `Result<float, E>` — is one inference never settled: no value of the
    /// variant carrying it exists. And inside a function type's signature a
    /// module-private type is written by the name its declaration writes
    /// rather than by its identity.
    pub fn binds_every_name(&self, ty: &Type) -> bool {
        !self.spells_unbound(&ty.kind, Spelling::Written)
    }

    fn spells_unbound(&self, kind: &TypeKind, spelling: Spelling) -> bool {
        spells_where(kind, &|kind| self.visit_name(kind, spelling))
    }

    /// The walk's step at `kind`: a function type's signature is read under
    /// its own spelling, an enum's arguments position by position.
    fn visit_name(&self, kind: &TypeKind, spelling: Spelling) -> Visit {
        if let (TypeKind::Function(_), Spelling::Written) = (kind, spelling) {
            return answered(self.spells_unbound(kind, Spelling::InSignature));
        }
        if let TypeKind::Custom(name, Some(args)) = kind {
            if let Some(params) = self.enum_parameters(name) {
                return answered(self.spells_unbound_argument(&params, args, spelling));
            }
        }
        if self.is_unbound_name(kind, spelling) {
            Visit::Applies
        } else {
            Visit::Descends
        }
    }

    /// Whether an argument of an enum declaring `params` names something
    /// unbound, an argument left at its own position's parameter aside.
    fn spells_unbound_argument(
        &self,
        params: &[&str],
        args: &[Expression],
        spelling: Spelling,
    ) -> bool {
        args.iter().enumerate().any(|(index, arg)| {
            let ExpressionKind::Type(ty, _) = &arg.node else {
                return false;
            };
            let is_unsettled = params
                .get(index)
                .is_some_and(|param| bare_type_name(&ty.kind) == Some(*param));
            !is_unsettled && self.spells_unbound(&ty.kind, spelling)
        })
    }

    /// The parameter names of the enum `name`, when the program defines one.
    fn enum_parameters(&self, name: &str) -> Option<Vec<&str>> {
        let Some(TypeDefinition::Enum(definition)) = self.definitions.get(name) else {
            return None;
        };
        Some(
            definition
                .generics
                .iter()
                .flatten()
                .map(|g| g.name.as_str())
                .collect(),
        )
    }

    /// Whether `kind` is a parameter or a type name nothing binds.
    fn is_unbound_name(&self, kind: &TypeKind, spelling: Spelling) -> bool {
        let is_param =
            |name: &String| self.type_params.contains(name) || self.open_params.contains(name);
        if let TypeKind::Generic(name, _, _) = kind {
            return !is_param(name);
        }
        let TypeKind::Custom(name, None) = kind else {
            return false;
        };
        !self.definitions.contains_key(name)
            && !is_param(name)
            && BuiltinCollectionKind::from_name(name).is_none()
            && name != crate::ast::types::SELF_TYPE_NAME
            && !(matches!(spelling, Spelling::InSignature) && self.names_private_type(name))
    }

    /// Whether `name` is the name a module-private type's declaration writes.
    fn names_private_type(&self, name: &str) -> bool {
        self.definitions
            .keys()
            .any(|identity| identity != name && type_identity::source_name(identity) == name)
    }

    /// Payload field `index` of `variant` of the enum `enum_name`, at the
    /// arguments the enum `instance` is written with.
    fn variant_field(
        &self,
        enum_name: &str,
        variant: &str,
        instance: &Type,
        index: usize,
    ) -> Option<Type> {
        let Some(TypeDefinition::Enum(definition)) = self.definitions.get(enum_name) else {
            return None;
        };
        let declared = definition.variants.get(variant)?.get(index)?;
        let TypeKind::Custom(_, Some(args)) = &instance.kind else {
            return Some(declared.clone());
        };
        let params = definition
            .generics
            .iter()
            .flatten()
            .map(|g| g.name.as_str());
        Some(crate::mir::instantiation::instantiated_member_type(
            params, args, declared,
        ))
    }

    /// The declared type of what `place` reaches, carried along every
    /// projection, or `None` where the walk cannot resolve one: behind a
    /// dereference, or past an element or field whose type the body does not
    /// record.
    ///
    /// The walk decides which projection applies over [`MirType`], which
    /// stores collection element types resolved. Alongside it the declared
    /// type is carried through every projection, because `MirType::Custom`
    /// names a class without its type arguments.
    pub fn declared(&self, place: &Place) -> Option<Type> {
        let decl = self.local_decls.get(place.local.0)?;
        let mut current = decl.mir_ty.clone();
        let mut declared = decl.ty.clone();
        for elem in &place.projection {
            let (next, next_declared) = match elem {
                PlaceElem::Deref => None,
                PlaceElem::Index(_) => project_index(&current, &declared),
                PlaceElem::Field(i) => self.project_field(place, &current, &declared, *i),
            }?;
            current = next;
            declared = next_declared;
        }
        Some(declared)
    }

    /// Field `index` of `name` as this body records it, read at the
    /// arguments `declared` writes, when the instance's own reading leaves a
    /// name unbound. Only reference counting asks for it — it still needs an
    /// answer for a field it has to release; a slot-type question gets none.
    fn recorded_field(&self, name: &str, declared: &Type, index: usize) -> Option<Type> {
        if self.strict {
            return None;
        }
        let declared_field = self.field_types.get(name)?.get(index)?;
        Some(field_type_in_instance(
            self.class_type_params,
            name,
            Some(declared),
            declared_field,
        ))
    }

    /// The type a `Field(index)` projection yields from a value of type `current`,
    /// declared `declared`, paired with its declared type; `None` when the field
    /// cannot be resolved.
    fn project_field(
        &self,
        place: &Place,
        current: &MirType,
        declared: &Type,
        index: usize,
    ) -> Option<(MirType, Type)> {
        match current {
            // Option<T>.Field(0) → the inner type T
            MirType::Option(inner) if index == 0 => {
                let TypeKind::Option(payload) = &declared.kind else {
                    return None;
                };
                Some((*inner.clone(), (**payload).clone()))
            }
            // Tuple(T0, T1, …).Field(i) → Ti
            MirType::Tuple(elems) => {
                let TypeKind::Tuple(elements) = &declared.kind else {
                    return None;
                };
                Some((
                    elems.get(index)?.clone(),
                    element_type(elements.get(index)?)?,
                ))
            }
            // Custom(struct/class).Field(i) → the declared field type, read at the
            // instance's type arguments.
            MirType::Custom(name) | MirType::Vector(name) => {
                let field_ty = match self.instance_field(declared, index) {
                    Some(field_ty) => field_ty,
                    None => self.recorded_field(name, declared, index)?,
                };
                Some((MirType::from_type_kind(&field_ty.kind), field_ty))
            }
            // Closure.Field(i) → the type of captured variable i.
            // Capture types are indexed by the root local because a closure
            // is always a single-level projection (never nested).
            MirType::Function => {
                let capture_ty = self.closure_capture_types.get(&place.local)?.get(index)?;
                Some((
                    MirType::from_type_kind(&capture_ty.kind),
                    capture_ty.clone(),
                ))
            }
            MirType::Option(_)
            | MirType::Int
            | MirType::I8
            | MirType::I16
            | MirType::I32
            | MirType::I64
            | MirType::I128
            | MirType::U8
            | MirType::U16
            | MirType::U32
            | MirType::U64
            | MirType::U128
            | MirType::Float
            | MirType::F16
            | MirType::F32
            | MirType::F64
            | MirType::Boolean
            | MirType::Void
            | MirType::Identifier
            | MirType::RawPtr
            | MirType::Error
            | MirType::String
            | MirType::List(_)
            | MirType::Array(_)
            | MirType::Map(_, _)
            | MirType::Set(_)
            | MirType::Result(_, _)
            | MirType::Future(_)
            | MirType::Generic
            | MirType::Unknown => None,
        }
    }
}

/// The element an `Index` projection yields from a collection of type
/// `current`, declared `declared`; `None` when it is no collection, or when
/// its element is a vector, which lives inline in the collection's buffer:
/// indexing yields an interior address rather than a pointer to an
/// allocation of its own, and retaining or releasing it would reach into the
/// buffer's bytes.
fn project_index(current: &MirType, declared: &Type) -> Option<(MirType, Type)> {
    if let Some(element) = tuple_element(declared) {
        return Some((MirType::from_type_kind(&element.kind), element));
    }
    let (MirType::Array(element)
    | MirType::List(element)
    | MirType::Set(element)
    | MirType::Map(_, element)) = current
    else {
        return None;
    };
    if element.is_inline_vector() {
        return None;
    }
    Some((*element.clone(), indexed_element_type(declared)?))
}

/// The element an `Index` projection yields from a tuple picked by a value:
/// a tuple, homogeneous whenever it is indexed so, yields its first element's
/// type, and the tuple class its methods take one as yields its argument.
fn tuple_element(declared: &Type) -> Option<Type> {
    if let TypeKind::Tuple(elements) = &declared.kind {
        return element_type(elements.first()?);
    }
    let TypeKind::Custom(name, Some(args)) = &declared.kind else {
        return None;
    };
    if name != TUPLE_TYPE_NAME {
        return None;
    }
    element_type(args.first()?)
}

/// The declared type an `Index` projection yields from a collection.
///
/// The walk's `MirType` keeps a collection's element resolved, but only the
/// declared spelling still carries a class element's type arguments — a
/// `List<Tagged<String>>` indexes to `Tagged<String>`, where the `MirType` is
/// `Custom("Tagged")` and names the class alone. Carrying the argument through
/// is what lets a field declared at the class parameter be read at the type it
/// actually holds. Indexing a sequence or a set yields its element; indexing a
/// map yields its value.
fn indexed_element_type(collection: &Type) -> Option<Type> {
    element_type(collection_element(&collection.kind)?)
}

/// The element expression of a collection type, or its value for a map;
/// `None` for every other kind.
fn collection_element(kind: &TypeKind) -> Option<&Expression> {
    match kind {
        TypeKind::Custom(name, Some(args)) => match BuiltinCollectionKind::from_name(name)? {
            BuiltinCollectionKind::Array
            | BuiltinCollectionKind::List
            | BuiltinCollectionKind::Set => args.first(),
            BuiltinCollectionKind::Map => args.get(1),
        },
        TypeKind::List(element) | TypeKind::Array(element, _) | TypeKind::Set(element) => {
            Some(element)
        }
        TypeKind::Map(_, value) => Some(value),
        TypeKind::Custom(_, None)
        | TypeKind::Int
        | TypeKind::I8
        | TypeKind::I16
        | TypeKind::I32
        | TypeKind::I64
        | TypeKind::I128
        | TypeKind::U8
        | TypeKind::U16
        | TypeKind::U32
        | TypeKind::U64
        | TypeKind::U128
        | TypeKind::Float
        | TypeKind::F16
        | TypeKind::F32
        | TypeKind::F64
        | TypeKind::String
        | TypeKind::Boolean
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::OneOf(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Option(_)
        | TypeKind::Future(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Function(_)
        | TypeKind::Meta(_)
        | TypeKind::Linear(_) => None,
    }
}

/// The type an element expression of a collection or tuple type denotes: a
/// written type, or a bare name, which names a type as written.
fn element_type(element: &Expression) -> Option<Type> {
    if let ExpressionKind::Identifier(name, _) = &element.node {
        return Some(Type::new(
            TypeKind::Custom(name.clone(), None),
            element.span,
        ));
    }
    type_argument(element)
}

/// Where a type the bound-name walk reads is written.
#[derive(Clone, Copy)]
enum Spelling {
    /// As a value's own type.
    Written,
    /// Inside a function type's signature.
    InSignature,
}

/// The walk's answer for a type whose verdict is already decided below it.
fn answered(spells_unbound: bool) -> Visit {
    if spells_unbound {
        Visit::Applies
    } else {
        Visit::Stops
    }
}

/// The name a bare type spells, parameter or type.
fn bare_type_name(kind: &TypeKind) -> Option<&str> {
    let (TypeKind::Custom(name, None) | TypeKind::Generic(name, None, _)) = kind else {
        return None;
    };
    Some(name)
}

/// Whether `kind` names one of the `open` type parameters; see
/// [`PlaceTypes::names_open_parameter`].
pub fn names_open_parameter(kind: &TypeKind, open: &std::collections::HashSet<String>) -> bool {
    crate::type_checker::instantiation_requirements::spells_a_type(kind, &|kind| {
        matches!(kind, TypeKind::Generic(name, _, _) if open.contains(name))
            || matches!(kind, TypeKind::Custom(name, None) if open.contains(name))
    })
}

/// The parameter types of every body of a program, by the link name a call
/// to it names: the one table every call's parameter slots are read from.
#[derive(Debug, Default)]
pub struct CalleeParameters {
    by_link_name: HashMap<String, Vec<Type>>,
}

impl CalleeParameters {
    /// The parameters of every body in `program`, `link_name` naming each
    /// body the way a call to it names its callee.
    pub fn of<N>(program: &[(N, Body)], link_name: impl Fn(&N) -> String) -> Self {
        let by_link_name = program
            .iter()
            .map(|(name, body)| {
                let params = (1..=body.arg_count)
                    .filter_map(|index| body.local_decls.get(index))
                    .map(|decl| decl.ty.clone())
                    .collect();
                (link_name(name), params)
            })
            .collect();
        Self { by_link_name }
    }

    /// The parameter types of the body a call to `symbol` reaches.
    pub fn of_callee(&self, symbol: &Symbol) -> Option<Vec<Type>> {
        self.by_link_name.get(&symbol.link_name()).cloned()
    }
}

/// What one argument of a call is passed as, as far as MIR spells it.
#[derive(Debug, Clone, PartialEq)]
pub enum ArgumentSlot {
    /// A parameter, element, key or value declared at this type.
    Declared(Type),
    /// A word no optional held as its bytes is passed in: a runtime entry
    /// point's own parameter, a receiver, the trailing allocator.
    Word,
    /// A parameter whose declared type the body cannot read.
    Unknown,
}

impl PlaceTypes<'_> {
    /// What each argument of the call `kind` is passed as, by position.
    ///
    /// A runtime entry point takes its elements, keys and values at the
    /// receiver collection's type arguments ([`crate::runtime_fns::element_roles`])
    /// and every other argument as a word; a Miri function at its parameters; a function value at the parameters its
    /// type declares; a virtual call at the parameter types its call site
    /// recorded. An argument past the declared parameters — the allocator —
    /// is a word. `callees` names every Miri function's parameters.
    pub fn call_argument_slots(
        &self,
        kind: &TerminatorKind,
        callees: &CalleeParameters,
    ) -> Vec<ArgumentSlot> {
        match kind {
            TerminatorKind::Call { func, args, .. } => self.call_slots(func, args, callees),
            TerminatorKind::VirtualCall {
                args, param_types, ..
            } => (0..args.len())
                .map(|position| match position.checked_sub(1) {
                    None => ArgumentSlot::Word,
                    Some(index) => match param_types.get(index) {
                        Some(Some(ty)) => ArgumentSlot::Declared(ty.clone()),
                        Some(None) => ArgumentSlot::Unknown,
                        None => ArgumentSlot::Word,
                    },
                })
                .collect(),
            TerminatorKind::GpuLaunch { .. }
            | TerminatorKind::Goto { .. }
            | TerminatorKind::SwitchInt { .. }
            | TerminatorKind::Return
            | TerminatorKind::Unreachable => Vec::new(),
        }
    }

    fn call_slots(
        &self,
        func: &Operand,
        args: &[Operand],
        callees: &CalleeParameters,
    ) -> Vec<ArgumentSlot> {
        let declared = match func {
            Operand::Function(function) => {
                if let Some(runtime) = function.symbol.runtime_name() {
                    return self.element_argument_slots(runtime, args);
                }
                callees.of_callee(&function.symbol)
            }
            Operand::Copy(place) | Operand::Move(place) => {
                self.declared(place).and_then(|ty| function_parameters(&ty))
            }
            Operand::Constant(_) => None,
        };
        let Some(params) = declared else {
            return vec![ArgumentSlot::Unknown; args.len()];
        };
        (0..args.len())
            .map(|position| match params.get(position) {
                Some(ty) => ArgumentSlot::Declared(ty.clone()),
                None => ArgumentSlot::Word,
            })
            .collect()
    }

    /// The runtime entry point `runtime`'s arguments: each element, key or
    /// value at the receiver collection's type argument, every other one a
    /// word.
    fn element_argument_slots(&self, runtime: &str, args: &[Operand]) -> Vec<ArgumentSlot> {
        let collection = args
            .first()
            .and_then(|receiver| self.operand_type(receiver));
        let mut slots = vec![ArgumentSlot::Word; args.len()];
        for &(position, role) in crate::runtime_fns::element_roles(runtime) {
            let slot = collection
                .as_ref()
                .and_then(|collection| element_slot(collection, role))
                .map_or(ArgumentSlot::Unknown, ArgumentSlot::Declared);
            if let Some(entry) = slots.get_mut(position) {
                *entry = slot;
            }
        }
        slots
    }
}

/// The parameter types a function type `ty` declares.
fn function_parameters(ty: &Type) -> Option<Vec<Type>> {
    let TypeKind::Function(function) = &ty.kind else {
        return None;
    };
    function
        .params
        .iter()
        .map(|param| type_argument(&param.typ))
        .collect()
}

/// The payload type of the optional `ty`, written either way.
fn optional_payload(ty: &Type) -> Option<Type> {
    if let TypeKind::Option(payload) = &ty.kind {
        return Some((**payload).clone());
    }
    let TypeKind::Custom(name, Some(args)) = &ty.kind else {
        return None;
    };
    let [payload] = args.as_slice() else {
        return None;
    };
    (name == crate::ast::types::OPTION_TYPE_NAME)
        .then(|| type_argument(payload))
        .flatten()
}

/// Element `slot` of the tuple `ty`.
fn tuple_member(ty: &Type, slot: usize) -> Option<Type> {
    let TypeKind::Tuple(elements) = &ty.kind else {
        return None;
    };
    element_type(elements.get(slot)?)
}

/// The type the collection `collection` holds in `role`: its element, its
/// keys or its values; `None` for a role it has none of.
pub fn element_slot(collection: &Type, role: ElementRole) -> Option<Type> {
    let (kind, arguments) = collection_arguments(collection)?;
    let position = match (kind, role) {
        (BuiltinCollectionKind::Map, ElementRole::Key)
        | (
            BuiltinCollectionKind::Array | BuiltinCollectionKind::List | BuiltinCollectionKind::Set,
            ElementRole::Element,
        ) => 0,
        (BuiltinCollectionKind::Map, ElementRole::Value) => 1,
        (BuiltinCollectionKind::Map, ElementRole::Element)
        | (
            BuiltinCollectionKind::Array | BuiltinCollectionKind::List | BuiltinCollectionKind::Set,
            ElementRole::Key | ElementRole::Value,
        ) => return None,
    };
    element_type(arguments.get(position)?)
}

/// Which built-in collection `ty` is, and the type arguments it is written
/// with, in either spelling.
fn collection_arguments(ty: &Type) -> Option<(BuiltinCollectionKind, Vec<&Expression>)> {
    if let TypeKind::Custom(name, Some(args)) = &ty.kind {
        return Some((
            BuiltinCollectionKind::from_name(name)?,
            args.iter().collect(),
        ));
    }
    if let TypeKind::List(element) = &ty.kind {
        return Some((BuiltinCollectionKind::List, vec![element.as_ref()]));
    }
    if let TypeKind::Array(element, _) = &ty.kind {
        return Some((BuiltinCollectionKind::Array, vec![element.as_ref()]));
    }
    if let TypeKind::Set(element) = &ty.kind {
        return Some((BuiltinCollectionKind::Set, vec![element.as_ref()]));
    }
    if let TypeKind::Map(key, value) = &ty.kind {
        return Some((
            BuiltinCollectionKind::Map,
            vec![key.as_ref(), value.as_ref()],
        ));
    }
    None
}
