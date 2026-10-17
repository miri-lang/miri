// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The value-class authority: whether a value of a type is a scalar, a run of
//! bytes with no managed interior, a single word whose owner decides its
//! treatment, or a reference-counted allocation.
//!
//! A struct, tuple or optional is a *bitwise value* when every field, element
//! or payload — read at the arguments the type is written with and through
//! every alias — is a scalar or itself a bitwise value, and the whole fits in
//! [`AUTO_COPY_MAX_SIZE`] bytes. Its [`ValueLayout`] places each member at its
//! natural alignment in declaration order, the way a C compiler lays out a
//! struct, and nests a member value's layout inline.
//!
//! Type arguments are never substituted into a rewritten type. A definition's
//! members are read in a frame binding its parameters to the arguments as
//! written, each argument judged in the frame it was written in, so a bare
//! name means what it means where it appears: inside a body, a name the body
//! leaves open is that parameter; inside a definition, a name is either one
//! of the definition's own parameters or a type from the table.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::ast::types::{
    vec_type_dim, BuiltinCollectionKind, Type, TypeKind, ATOMIC_TYPE_NAME, AUTO_COPY_MAX_SIZE,
};
use crate::type_checker::context::{GenericDefinition, StructDefinition, TypeDefinition};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The width of `int` and of every pointer-width word, in bytes.
const WORD_BYTES: u32 = 8;

/// How many types one classification may visit, and how deeply it may nest,
/// before it answers `Managed`.
///
/// A struct that holds itself through an optional or an alias chain is an
/// infinite type; these bounds are what stop the walk over it, the depth
/// before the stack runs out and the visit count before a type that fans out
/// at every level does. A bitwise value is at most [`AUTO_COPY_MAX_SIZE`]
/// bytes, so a legitimate one comes nowhere near either.
const VISIT_BUDGET: u32 = 4096;
const DEPTH_LIMIT: u32 = 64;

/// What the layout of a type is decided against: the program's type table and
/// the type parameters the body being laid out leaves open.
///
/// Inside a generic body a bare name that is one of its parameters means that
/// parameter, even where the program defines a type of the same name, so a
/// standard-library body over `V` is not laid out as the program's own `V`.
#[derive(Debug, Clone, Copy)]
pub struct LayoutScope<'d> {
    pub definitions: &'d HashMap<String, TypeDefinition>,
    pub type_params: &'d HashSet<String>,
}

impl<'d> LayoutScope<'d> {
    /// The scope of a body that leaves `type_params` open.
    pub fn new(
        definitions: &'d HashMap<String, TypeDefinition>,
        type_params: &'d HashSet<String>,
    ) -> Self {
        Self {
            definitions,
            type_params,
        }
    }

    /// The scope of code with no type parameter open.
    pub fn program(definitions: &'d HashMap<String, TypeDefinition>) -> Self {
        static NO_PARAMETERS: OnceLock<HashSet<String>> = OnceLock::new();
        Self::new(definitions, NO_PARAMETERS.get_or_init(HashSet::new))
    }

    /// Whether the bare name `name` is one of the open type parameters.
    pub fn is_parameter(&self, name: &str) -> bool {
        self.type_params.contains(name)
    }
}

/// A scalar, by its representation; each width is in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ScalarKind {
    Signed(u32),
    Unsigned(u32),
    Float(u32),
    Boolean,
}

impl ScalarKind {
    /// The scalar's size in bytes, which is also its alignment.
    pub fn size(self) -> u32 {
        match self {
            ScalarKind::Signed(bytes) | ScalarKind::Unsigned(bytes) | ScalarKind::Float(bytes) => {
                bytes
            }
            ScalarKind::Boolean => 1,
        }
    }
}

/// Why a value is held as one pointer-width word.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WordRole {
    /// A type parameter left open: the instantiation decides what the word
    /// holds, so the word itself is never counted.
    Parameter,
    /// An optional held as its payload's reference-counted pointer, null when
    /// absent. The word is counted like the payload it points at.
    NullablePointer,
    /// A word no reference count is kept for: a raw pointer, an identifier,
    /// and the kinds that name no stored value (a metatype, a type set, the
    /// error type).
    Untracked,
}

/// How a value of a type is represented.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ValueClass {
    /// A single scalar, alias spellings included.
    Scalar(ScalarKind),
    /// A struct, tuple or optional held as bytes with no managed interior.
    Bitwise(ValueLayout),
    /// One pointer-width word that is neither a scalar nor an allocation of
    /// this type's own.
    Word(WordRole),
    /// A reference-counted allocation, or anything held as one.
    Managed,
}

/// The bytes of a bitwise value: its size, its alignment, and where each
/// member sits, in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueLayout {
    pub size: u32,
    pub align: u32,
    pub fields: Vec<ValueField>,
}

/// One member of a [`ValueLayout`] at its byte offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueField {
    pub offset: u32,
    pub repr: FieldRepr,
}

/// What a [`ValueField`] holds.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum FieldRepr {
    Scalar(ScalarKind),
    /// A bitwise value nested inline.
    Value(ValueLayout),
    /// An optional's one-byte presence tag: zero when absent.
    OptionTag,
}

impl FieldRepr {
    fn size_and_align(&self) -> (u32, u32) {
        match self {
            FieldRepr::Scalar(scalar) => (scalar.size(), scalar.size()),
            FieldRepr::Value(layout) => (layout.size, layout.align),
            FieldRepr::OptionTag => (1, 1),
        }
    }
}

/// The value class of `kind`, decided against `scope`.
///
/// Inline vectors keep the representation they have today, a reference-counted
/// allocation of their own, and so answer `Managed`; an enum answers `Managed`
/// whatever its payloads.
pub fn value_class(kind: &TypeKind, scope: LayoutScope<'_>) -> ValueClass {
    let mut classifier = Classifier {
        definitions: scope.definitions,
        visits_left: VISIT_BUDGET,
        depth: 0,
    };
    classifier
        .classify(kind, Names::Body(scope.type_params))
        .into_class()
}

/// A classification on its way out: the public class, or a managed value
/// that is the pointer to its own allocation. The difference decides whether
/// an optional around it can be that pointer, null when absent.
enum Verdict {
    Class(ValueClass),
    Pointer,
}

impl Verdict {
    fn into_class(self) -> ValueClass {
        match self {
            Verdict::Class(class) => class,
            Verdict::Pointer => ValueClass::Managed,
        }
    }

    fn managed() -> Self {
        Verdict::Class(ValueClass::Managed)
    }
}

/// How a bare name is read where it is written.
#[derive(Clone, Copy)]
enum Names<'n, 'a> {
    /// In the body being laid out: a name it leaves open is that parameter.
    Body(&'a HashSet<String>),
    /// Inside a definition: its own parameters, bound to their arguments.
    Definition(&'n [Binding<'n, 'a>]),
}

impl<'n, 'a> Names<'n, 'a> {
    /// `Some` when `name` is a type parameter here, holding its argument
    /// when one was written.
    fn parameter(self, name: &str) -> Option<Option<Argument<'n, 'a>>> {
        match self {
            Names::Body(open) => open.contains(name).then_some(None),
            Names::Definition(bindings) => bindings
                .iter()
                .find(|binding| binding.parameter == name)
                .map(|binding| binding.argument),
        }
    }
}

/// A definition's type parameter and the argument it was written with.
#[derive(Clone, Copy)]
struct Binding<'n, 'a> {
    parameter: &'a str,
    argument: Option<Argument<'n, 'a>>,
}

/// A type as written, with the names it is read in.
#[derive(Clone, Copy)]
struct Argument<'n, 'a> {
    ty: &'a Type,
    is_nullable: bool,
    names: Names<'n, 'a>,
}

impl<'n, 'a> Argument<'n, 'a> {
    /// The argument `expression` spells, or `None` for a value argument.
    fn written(expression: &'a Expression, names: Names<'n, 'a>) -> Option<Self> {
        let ExpressionKind::Type(ty, is_nullable) = &expression.node else {
            return None;
        };
        Some(Self {
            ty,
            is_nullable: *is_nullable,
            names,
        })
    }

    fn plain(ty: &'a Type, names: Names<'n, 'a>) -> Self {
        Self {
            ty,
            is_nullable: false,
            names,
        }
    }
}

/// The frame of a definition with `generics`, written with `args` in `names`.
fn bind<'n, 'a>(
    generics: Option<&'a [GenericDefinition]>,
    args: Option<&'a [Expression]>,
    names: Names<'n, 'a>,
) -> Vec<Binding<'n, 'a>> {
    let args = args.unwrap_or_default();
    generics
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(position, generic)| Binding {
            parameter: generic.name.as_str(),
            argument: args
                .get(position)
                .and_then(|arg| Argument::written(arg, names)),
        })
        .collect()
}

struct Classifier<'a> {
    definitions: &'a HashMap<String, TypeDefinition>,
    visits_left: u32,
    depth: u32,
}

impl<'a> Classifier<'a> {
    fn classify(&mut self, kind: &'a TypeKind, names: Names<'_, 'a>) -> Verdict {
        if self.visits_left == 0 || self.depth == DEPTH_LIMIT {
            return Verdict::managed();
        }
        self.visits_left -= 1;
        self.depth += 1;
        let verdict = self.classify_kind(kind, names);
        self.depth -= 1;
        verdict
    }

    fn classify_kind(&mut self, kind: &'a TypeKind, names: Names<'_, 'a>) -> Verdict {
        match kind {
            TypeKind::Tuple(elements) => self.classify_tuple(elements, names),
            TypeKind::Option(payload) => self.classify_option(Argument::plain(payload, names)),
            TypeKind::Generic(name, _, _) => self.classify_parameter(names.parameter(name)),
            TypeKind::Custom(name, args) => self.classify_custom(kind, name, args, names),
            TypeKind::Void => Verdict::Class(ValueClass::Bitwise(ValueLayout::empty())),
            TypeKind::String
            | TypeKind::List(_)
            | TypeKind::Array(_, _)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Function(_) => Verdict::Pointer,
            TypeKind::Linear(_) | TypeKind::Result(_, _) | TypeKind::Future(_) => {
                Verdict::managed()
            }
            TypeKind::RawPtr
            | TypeKind::Identifier
            | TypeKind::Meta(_)
            | TypeKind::OneOf(_)
            | TypeKind::Error => Verdict::Class(ValueClass::Word(WordRole::Untracked)),
            TypeKind::Int
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
            | TypeKind::Boolean => match scalar_kind(kind) {
                Some(scalar) => Verdict::Class(ValueClass::Scalar(scalar)),
                None => Verdict::managed(),
            },
        }
    }

    fn classify_argument(&mut self, argument: Argument<'_, 'a>) -> Verdict {
        if argument.is_nullable {
            return self.classify_option(Argument {
                is_nullable: false,
                ..argument
            });
        }
        self.classify(&argument.ty.kind, argument.names)
    }

    /// A type parameter: its argument's class, or an open word.
    fn classify_parameter(&mut self, parameter: Option<Option<Argument<'_, 'a>>>) -> Verdict {
        match parameter {
            Some(Some(argument)) => self.classify_argument(argument),
            Some(None) | None => Verdict::Class(ValueClass::Word(WordRole::Parameter)),
        }
    }

    fn classify_custom(
        &mut self,
        kind: &'a TypeKind,
        name: &'a str,
        args: &'a Option<Vec<Expression>>,
        names: Names<'_, 'a>,
    ) -> Verdict {
        if args.is_none() {
            if let Some(parameter) = names.parameter(name) {
                return self.classify_parameter(Some(parameter));
            }
        }
        if vec_type_dim(kind).is_some() {
            return Verdict::managed();
        }
        if let Some(component) = atomic_component(name, args.as_deref(), names) {
            return self.classify_argument(component);
        }
        if BuiltinCollectionKind::from_name(name).is_some() {
            return collection_verdict(args.is_some());
        }
        self.classify_definition(name, args.as_deref(), names)
    }

    fn classify_definition(
        &mut self,
        name: &str,
        args: Option<&'a [Expression]>,
        names: Names<'_, 'a>,
    ) -> Verdict {
        match self.definitions.get(name) {
            Some(TypeDefinition::Struct(definition)) => {
                self.classify_struct(definition, args, names)
            }
            Some(TypeDefinition::Alias(alias)) => {
                let frame = bind(alias.generics.as_deref(), args, names);
                self.classify(&alias.template.kind, Names::Definition(&frame))
            }
            Some(TypeDefinition::Generic(_)) => {
                Verdict::Class(ValueClass::Word(WordRole::Parameter))
            }
            Some(TypeDefinition::Class(_) | TypeDefinition::Enum(_) | TypeDefinition::Trait(_)) => {
                Verdict::Pointer
            }
            None => Verdict::managed(),
        }
    }

    fn classify_struct(
        &mut self,
        definition: &'a StructDefinition,
        args: Option<&'a [Expression]>,
        names: Names<'_, 'a>,
    ) -> Verdict {
        let frame = bind(definition.generics.as_deref(), args, names);
        let members = definition
            .fields
            .iter()
            .map(|(_, ty, _)| Some(Argument::plain(ty, Names::Definition(&frame))))
            .collect();
        self.classify_aggregate(members)
    }

    fn classify_tuple(&mut self, elements: &'a [Expression], names: Names<'_, 'a>) -> Verdict {
        let members = elements
            .iter()
            .map(|element| Argument::written(element, names))
            .collect();
        self.classify_aggregate(members)
    }

    /// A struct or tuple: bitwise when every member is, otherwise its own
    /// allocation. `None` marks a member that is not a type.
    fn classify_aggregate(&mut self, members: Vec<Option<Argument<'_, 'a>>>) -> Verdict {
        let mut layout = LayoutBuilder::default();
        for member in members {
            let Some(repr) = member.and_then(|argument| self.member_repr(argument)) else {
                return Verdict::Pointer;
            };
            layout.push(repr);
        }
        match layout.finish() {
            Some(layout) => Verdict::Class(ValueClass::Bitwise(layout)),
            None => Verdict::Pointer,
        }
    }

    /// How `argument` sits inside a bitwise value, or `None` when it cannot.
    fn member_repr(&mut self, argument: Argument<'_, 'a>) -> Option<FieldRepr> {
        match self.classify_argument(argument) {
            Verdict::Class(ValueClass::Scalar(scalar)) => Some(FieldRepr::Scalar(scalar)),
            Verdict::Class(ValueClass::Bitwise(layout)) => Some(FieldRepr::Value(layout)),
            Verdict::Class(ValueClass::Word(_) | ValueClass::Managed) | Verdict::Pointer => None,
        }
    }

    /// An optional of `payload`: a tag followed by the payload when the
    /// payload is a value, the payload's own pointer when it is one, and a
    /// boxed layer otherwise — over a word, over an optional held as a
    /// pointer, and over managed kinds that are no pointer of their own.
    fn classify_option(&mut self, payload: Argument<'_, 'a>) -> Verdict {
        let payload_repr = match self.classify_argument(payload) {
            Verdict::Class(ValueClass::Scalar(scalar)) => FieldRepr::Scalar(scalar),
            Verdict::Class(ValueClass::Bitwise(layout)) => FieldRepr::Value(layout),
            Verdict::Pointer => return Verdict::Class(ValueClass::Word(WordRole::NullablePointer)),
            Verdict::Class(ValueClass::Word(_) | ValueClass::Managed) => return Verdict::managed(),
        };
        let mut layout = LayoutBuilder::default();
        layout.push(FieldRepr::OptionTag);
        layout.push(payload_repr);
        match layout.finish() {
            Some(layout) => Verdict::Class(ValueClass::Bitwise(layout)),
            None => Verdict::managed(),
        }
    }
}

/// A built-in collection is its own allocation at its type arguments; named
/// bare, it is the unresolved self-reference inside the collection's own
/// body, which holds an element word.
fn collection_verdict(has_arguments: bool) -> Verdict {
    if has_arguments {
        Verdict::Pointer
    } else {
        Verdict::Class(ValueClass::Word(WordRole::Parameter))
    }
}

/// The component of the compiler-known atomic element type, which is held as
/// that component. A user type that only shares the name carries none.
fn atomic_component<'n, 'a>(
    name: &str,
    args: Option<&'a [Expression]>,
    names: Names<'n, 'a>,
) -> Option<Argument<'n, 'a>> {
    if name != ATOMIC_TYPE_NAME {
        return None;
    }
    Argument::written(args?.first()?, names)
}

/// The scalar `kind` is, or `None` for every kind that is not one.
fn scalar_kind(kind: &TypeKind) -> Option<ScalarKind> {
    let scalar = match kind {
        TypeKind::I8 => ScalarKind::Signed(1),
        TypeKind::I16 => ScalarKind::Signed(2),
        TypeKind::I32 => ScalarKind::Signed(4),
        TypeKind::I64 => ScalarKind::Signed(8),
        TypeKind::I128 => ScalarKind::Signed(16),
        TypeKind::Int => ScalarKind::Signed(WORD_BYTES),
        TypeKind::U8 => ScalarKind::Unsigned(1),
        TypeKind::U16 => ScalarKind::Unsigned(2),
        TypeKind::U32 => ScalarKind::Unsigned(4),
        TypeKind::U64 => ScalarKind::Unsigned(8),
        TypeKind::U128 => ScalarKind::Unsigned(16),
        TypeKind::F16 => ScalarKind::Float(2),
        TypeKind::F32 => ScalarKind::Float(4),
        TypeKind::F64 => ScalarKind::Float(8),
        TypeKind::Float => ScalarKind::Float(WORD_BYTES),
        TypeKind::Boolean => ScalarKind::Boolean,
        TypeKind::String
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Linear(_)
        | TypeKind::OneOf(_) => return None,
    };
    Some(scalar)
}

impl ValueLayout {
    /// The layout of a value with no members: nothing to read or write.
    fn empty() -> Self {
        Self {
            size: 0,
            align: 1,
            fields: Vec::new(),
        }
    }
}

/// Places members one after another, each at its natural alignment.
struct LayoutBuilder {
    end: u32,
    align: u32,
    fields: Vec<ValueField>,
}

impl Default for LayoutBuilder {
    fn default() -> Self {
        let empty = ValueLayout::empty();
        Self {
            end: empty.size,
            align: empty.align,
            fields: empty.fields,
        }
    }
}

impl LayoutBuilder {
    fn push(&mut self, repr: FieldRepr) {
        let (size, align) = repr.size_and_align();
        let offset = align_up(self.end, align);
        self.end = offset.saturating_add(size);
        self.align = self.align.max(align);
        self.fields.push(ValueField { offset, repr });
    }

    /// The finished layout, or `None` past the bitwise-value size limit.
    fn finish(self) -> Option<ValueLayout> {
        let size = align_up(self.end, self.align);
        let fits = usize::try_from(size).is_ok_and(|size| size <= AUTO_COPY_MAX_SIZE);
        fits.then_some(ValueLayout {
            size,
            align: self.align,
            fields: self.fields,
        })
    }
}

fn align_up(offset: u32, align: u32) -> u32 {
    offset.div_ceil(align.max(1)).saturating_mul(align.max(1))
}
