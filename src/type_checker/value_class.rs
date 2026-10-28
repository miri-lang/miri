// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The value-class authority: whether a value of a type is a scalar, a run of
//! bytes with no managed interior, a single word whose owner decides its
//! treatment, or a reference-counted allocation.
//!
//! A struct, tuple, optional or enum is a *bitwise value* when every field,
//! element or payload — read at the arguments the type is written with and
//! through every alias — is a scalar or itself a bitwise value, and the whole
//! fits in [`AUTO_COPY_MAX_SIZE`] bytes. Its [`ValueLayout`] places each member
//! at its natural alignment in declaration order, the way a C compiler lays out
//! a struct, and nests a member value's layout inline. An enum is its tag, one
//! word, followed by one column per payload position that every variant
//! storing a field there shares; an enum declaring a drop hook keeps its
//! identity and is never bitwise, nor is one supplying its own `equals`,
//! which a comparison of its bytes would bypass, and neither, for now, is a
//! generic one.
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
    EQUALS_METHOD_NAME, OPTION_TYPE_NAME, RANGE_TYPE_NAME,
};
use crate::type_checker::context::{
    EnumDefinition, GenericDefinition, StructDefinition, TypeDefinition,
};
use crate::type_checker::utils::has_drop_hook;
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

/// The width of `int` and of every pointer-width word, in bytes: the open
/// parameter's optional holds one beside its tag.
///
/// TODO: this is the 64-bit hosts' pointer width, fixed. A 32-bit target needs
/// the target's width carried in [`LayoutScope`] and read here, so the open
/// parameter's word and the payload codegen stores into it agree.
pub const WORD_BYTES: u32 = 8;

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

/// The bytes of a bitwise value: its size, its alignment, what kind of value
/// it lays out, and where each member sits, in declaration order.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ValueLayout {
    pub size: u32,
    pub align: u32,
    pub shape: LayoutShape,
    pub fields: Vec<ValueField>,
}

/// What kind of value a [`ValueLayout`] lays out, which decides how a field
/// projection numbers its members and how two values are compared.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayoutShape {
    /// A struct, tuple or range: each member numbered as it is laid out.
    Record,
    /// An optional: its presence tag, then its one member, the payload.
    Optional,
    /// An enum: its tag as member 0, then payload position `k` as member
    /// `k + 1`, each a [`FieldRepr::Overlay`] column.
    ///
    /// `variants` holds, for each variant in discriminant order — the order
    /// the definition keeps its variants in, sorted by name, not the order
    /// they are declared in — which member of each column its fields are:
    /// field `k` of the variant the tag `d` names is member `variants[d][k]`
    /// of column `k + 1`. A column read alone does not say which variant
    /// wrote it; the tag does.
    Enum { variants: Vec<Vec<usize>> },
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
    /// One payload position of an enum: what each variant carrying a field
    /// there stores in it, each distinct member once, in variant order. The
    /// column is as wide and as aligned as the widest member, and never less
    /// than a word, the column a reader of a counted enum loads a word from.
    Overlay(Vec<FieldRepr>),
}

impl FieldRepr {
    /// The bytes this member spans and the alignment it takes.
    pub fn size_and_align(&self) -> (u32, u32) {
        match self {
            FieldRepr::Scalar(scalar) => (scalar.size(), scalar.size()),
            FieldRepr::Value(layout) => (layout.size, layout.align),
            FieldRepr::OptionTag => (1, 1),
            FieldRepr::Overlay(members) => members.iter().map(FieldRepr::size_and_align).fold(
                (WORD_BYTES, WORD_BYTES),
                |(size, align), (member_size, member_align)| {
                    (size.max(member_size), align.max(member_align))
                },
            ),
        }
    }
}

/// The value class of `kind`, decided against `scope`.
///
/// Inline vectors keep the representation they have today, a reference-counted
/// allocation of their own, and so answer `Managed`.
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

/// Whether `kind` is the type an absent value is spelled at before it meets
/// a slot: optionals around nothing (`void?`, `void??`, ...). Such a value is
/// a word — null when absent, otherwise a box around the next layer — however
/// the slot it meets holds its optional; [`value_class`] classifies it so.
pub fn is_absent_placeholder(kind: &TypeKind) -> bool {
    let TypeKind::Option(payload) = kind else {
        return false;
    };
    payload.kind == TypeKind::Void || is_absent_placeholder(&payload.kind)
}

/// The layout a value of `kind` is held in as its own bytes, decided against
/// `scope`, or `None` when it is held as one word.
///
/// This is the one place that decides which bitwise values leave the heap. A
/// struct, tuple, optional or enum the authority calls bitwise is held as its
/// bytes:
/// in a frame's stack slot, inline in the class, struct or collection holding
/// it. An optional held so is its presence tag with the payload beside it. A
/// value of no bytes has nothing to hold.
pub fn held_value_layout(kind: &TypeKind, scope: LayoutScope<'_>) -> Option<ValueLayout> {
    match value_class(kind, scope) {
        ValueClass::Bitwise(layout) if layout.is_held_as_bytes() => Some(layout),
        ValueClass::Bitwise(_)
        | ValueClass::Scalar(_)
        | ValueClass::Word(_)
        | ValueClass::Managed => None,
    }
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
        if let Some(payload) = option_payload(name, args.as_deref(), names) {
            return self.classify_option(payload);
        }
        if vec_type_dim(kind).is_some() {
            return Verdict::managed();
        }
        if let Some(component) = atomic_component(name, args.as_deref(), names) {
            return self.classify_argument(component);
        }
        if let Some(element) = range_element(name, args.as_deref(), names) {
            return self.classify_range(element);
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
            Some(TypeDefinition::Enum(definition)) => {
                if has_drop_hook(name, self.definitions)
                    || supplies_own_equality(definition)
                    || is_generic_enum(definition)
                {
                    return Verdict::Pointer;
                }
                self.classify_enum(definition, args, names)
            }
            Some(TypeDefinition::Alias(alias)) => {
                let frame = bind(alias.generics.as_deref(), args, names);
                self.classify(&alias.template.kind, Names::Definition(&frame))
            }
            Some(TypeDefinition::Generic(_)) => {
                Verdict::Class(ValueClass::Word(WordRole::Parameter))
            }
            Some(TypeDefinition::Class(_) | TypeDefinition::Trait(_)) => Verdict::Pointer,
            None => Verdict::managed(),
        }
    }

    /// An enum without a drop hook: bitwise when every payload of every
    /// variant is, laid out as its tag and then one column per payload
    /// position, each overlaying what the variants store there.
    fn classify_enum(
        &mut self,
        definition: &'a EnumDefinition,
        args: Option<&'a [Expression]>,
        names: Names<'_, 'a>,
    ) -> Verdict {
        let frame = bind(definition.generics.as_deref(), args, names);
        let mut columns: Vec<Vec<FieldRepr>> = Vec::new();
        let mut variants = Vec::with_capacity(definition.variants.len());
        for payloads in definition.variants.values() {
            let mut members = Vec::with_capacity(payloads.len());
            for (position, payload) in payloads.iter().enumerate() {
                let argument = Argument::plain(payload, Names::Definition(&frame));
                let Some(repr) = self.member_repr(argument) else {
                    return Verdict::Pointer;
                };
                if columns.len() == position {
                    columns.push(Vec::new());
                }
                members.push(overlay_member(&mut columns[position], repr));
            }
            variants.push(members);
        }
        let mut layout = LayoutBuilder::new(LayoutShape::Enum { variants });
        layout.push(FieldRepr::Scalar(ScalarKind::Signed(WORD_BYTES)));
        for members in columns {
            layout.push(FieldRepr::Overlay(members));
        }
        match layout.finish() {
            Some(layout) => Verdict::Class(ValueClass::Bitwise(layout)),
            None => Verdict::Pointer,
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
        let mut layout = LayoutBuilder::new(LayoutShape::Record);
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

    /// A range over `element`: built as its start, its end and whether the
    /// end is included, a tuple of those three.
    fn classify_range(&mut self, element: Argument<'_, 'a>) -> Verdict {
        let mut layout = LayoutBuilder::new(LayoutShape::Record);
        for _ in 0..2 {
            let Some(repr) = self.member_repr(element) else {
                return Verdict::Pointer;
            };
            layout.push(repr);
        }
        layout.push(FieldRepr::Scalar(ScalarKind::Boolean));
        match layout.finish() {
            Some(layout) => Verdict::Class(ValueClass::Bitwise(layout)),
            None => Verdict::Pointer,
        }
    }

    /// An optional of `payload`: a tag followed by the payload when the
    /// payload is a scalar, a value or an open parameter's word; the payload's
    /// own pointer when it is one; and a boxed layer otherwise — over an
    /// optional held as a pointer, over an untracked word, and over managed
    /// kinds that are no pointer of their own.
    fn classify_option(&mut self, payload: Argument<'_, 'a>) -> Verdict {
        // An optional of nothing is the type an absent value is spelled at
        // before it meets a slot (see [`is_absent_placeholder`]). It keeps the
        // word every optional's slot reads as absent when null, whatever that
        // slot holds.
        if !payload.is_nullable && payload.ty.kind == TypeKind::Void {
            return Verdict::managed();
        }
        let payload_repr = match self.classify_argument(payload) {
            Verdict::Class(ValueClass::Scalar(scalar)) => FieldRepr::Scalar(scalar),
            Verdict::Class(ValueClass::Bitwise(layout)) => FieldRepr::Value(layout),
            // An open parameter is one uncounted word, whatever it is bound
            // to: its optional holds that word beside a tag, the way an
            // optional of `int` does.
            Verdict::Class(ValueClass::Word(WordRole::Parameter)) => {
                FieldRepr::Scalar(ScalarKind::Signed(WORD_BYTES))
            }
            Verdict::Pointer => return Verdict::Class(ValueClass::Word(WordRole::NullablePointer)),
            Verdict::Class(
                ValueClass::Word(WordRole::NullablePointer | WordRole::Untracked)
                | ValueClass::Managed,
            ) => return Verdict::managed(),
        };
        let mut layout = LayoutBuilder::new(LayoutShape::Optional);
        layout.push(FieldRepr::OptionTag);
        layout.push(payload_repr);
        match layout.finish() {
            Some(layout) => Verdict::Class(ValueClass::Bitwise(layout)),
            None => Verdict::managed(),
        }
    }
}

/// Whether `definition` declares type parameters.
///
/// TODO: a generic enum stays a counted allocation at every instantiation.
/// Its methods are not lowered at the instantiation their callers hold: a
/// `static fn` returning `Self` is one body shared by every instantiation,
/// whose result leaves the parameter open, and the enum's own name written
/// bare in a method's signature is not substituted with the instantiation the
/// way `Self` is. Holding `E<int>` as its bytes while those bodies hand over
/// a counted `E<T>` would meet one value in two representations. Both
/// lowering seams need the method bodies of a generic enum lowered at each
/// instantiation before this rule can go.
fn is_generic_enum(definition: &EnumDefinition) -> bool {
    definition
        .generics
        .as_ref()
        .is_some_and(|generics| !generics.is_empty())
}

/// Whether `definition` supplies its own `equals`, the method `==` on the
/// enum then calls rather than comparing payloads. Such an enum decides alone what equal means, which a
/// comparison of its bytes would bypass, so it stays counted and every `==`
/// reaching it calls that `equals`.
fn supplies_own_equality(definition: &EnumDefinition) -> bool {
    definition.methods.contains_key(EQUALS_METHOD_NAME)
}

/// The index of `repr` among the members of an enum column, added when no
/// variant stored it there before.
fn overlay_member(column: &mut Vec<FieldRepr>, repr: FieldRepr) -> usize {
    match column.iter().position(|member| *member == repr) {
        Some(index) => index,
        None => {
            column.push(repr);
            column.len() - 1
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

/// The payload `T` of an optional written as the generic type `Option<T>`,
/// which is the optional `T?` under another spelling: classified alike, so
/// no consumer of the authority meets two layouts for one type.
fn option_payload<'n, 'a>(
    name: &str,
    args: Option<&'a [Expression]>,
    names: Names<'n, 'a>,
) -> Option<Argument<'n, 'a>> {
    if name != OPTION_TYPE_NAME {
        return None;
    }
    let [payload] = args? else {
        return None;
    };
    Argument::written(payload, names)
}

/// The element `T` of the compiler-known range type `Range<T>`, whose value is
/// built as the tuple of its start, its end and whether the end is included.
fn range_element<'n, 'a>(
    name: &str,
    args: Option<&'a [Expression]>,
    names: Names<'n, 'a>,
) -> Option<Argument<'n, 'a>> {
    if name != RANGE_TYPE_NAME {
        return None;
    }
    let [element] = args? else {
        return None;
    };
    Argument::written(element, names)
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
    /// Whether a bitwise value laid out this way is held as its bytes rather
    /// than as a word (see [`held_value_layout`]): every one with bytes to
    /// hold.
    pub fn is_held_as_bytes(&self) -> bool {
        self.size > 0
    }

    /// Whether this is the layout of an optional: its presence tag, then its
    /// payload.
    pub fn is_optional(&self) -> bool {
        self.shape == LayoutShape::Optional
    }

    /// The member a field projection numbered `index` reaches.
    ///
    /// A struct's or tuple's members are numbered as they are laid out, and
    /// so are an enum's: its tag, then its payload columns. An optional's tag
    /// is no member of its own: its one member, numbered 0, is the payload
    /// beside the tag.
    pub fn projected_field(&self, index: usize) -> Option<&ValueField> {
        match self.shape {
            LayoutShape::Optional => (index == 0).then(|| self.fields.get(1)).flatten(),
            LayoutShape::Record | LayoutShape::Enum { .. } => self.fields.get(index),
        }
    }

    /// Whether two values laid out this way are equal exactly when their
    /// bytes are: no member is a float, whose `-0.0` and `0.0` are equal and
    /// whose NaN equals nothing, no byte is padding, whose contents are
    /// unspecified, and no member is an enum's payload column, whose bytes
    /// past the field its variant wrote are no part of the value.
    pub fn is_compared_by_bytes(&self) -> bool {
        !self.holds_float() && !self.holds_overlay() && self.member_bytes() == self.size
    }

    /// Whether a float appears anywhere in this layout.
    fn holds_float(&self) -> bool {
        self.fields.iter().any(|field| field.repr.holds_float())
    }

    /// Whether an enum payload column appears anywhere in this layout.
    fn holds_overlay(&self) -> bool {
        self.fields.iter().any(|field| field.repr.holds_overlay())
    }

    /// How many of this layout's bytes are members rather than padding.
    fn member_bytes(&self) -> u32 {
        self.fields
            .iter()
            .map(|field| match &field.repr {
                FieldRepr::Value(inner) => inner.member_bytes(),
                FieldRepr::Scalar(_) | FieldRepr::OptionTag | FieldRepr::Overlay(_) => {
                    field.repr.size_and_align().0
                }
            })
            .sum()
    }

    /// The layout of a value with no members: nothing to read or write.
    fn empty() -> Self {
        Self {
            size: 0,
            align: 1,
            shape: LayoutShape::Record,
            fields: Vec::new(),
        }
    }
}

impl FieldRepr {
    /// Whether a float appears anywhere in this member.
    fn holds_float(&self) -> bool {
        match self {
            FieldRepr::Scalar(ScalarKind::Float(_)) => true,
            FieldRepr::Value(inner) => inner.holds_float(),
            FieldRepr::Overlay(members) => members.iter().any(FieldRepr::holds_float),
            FieldRepr::Scalar(
                ScalarKind::Signed(_) | ScalarKind::Unsigned(_) | ScalarKind::Boolean,
            )
            | FieldRepr::OptionTag => false,
        }
    }

    /// Whether this member is, or nests, an enum payload column.
    fn holds_overlay(&self) -> bool {
        match self {
            FieldRepr::Overlay(_) => true,
            FieldRepr::Value(inner) => inner.holds_overlay(),
            FieldRepr::Scalar(_) | FieldRepr::OptionTag => false,
        }
    }
}

/// Places members one after another, each at its natural alignment.
struct LayoutBuilder {
    end: u32,
    align: u32,
    shape: LayoutShape,
    fields: Vec<ValueField>,
}

impl LayoutBuilder {
    /// A builder of a value of `shape` with no members yet.
    fn new(shape: LayoutShape) -> Self {
        let empty = ValueLayout::empty();
        Self {
            end: empty.size,
            align: empty.align,
            shape,
            fields: empty.fields,
        }
    }

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
            shape: self.shape,
            fields: self.fields,
        })
    }
}

fn align_up(offset: u32, align: u32) -> u32 {
    offset.div_ceil(align.max(1)).saturating_mul(align.max(1))
}
