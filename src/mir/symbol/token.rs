// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The token a type argument contributes to a symbol, and the grammar under
//! which two different types never share one.

use std::borrow::Cow;

use crate::ast::expression::Expression;
use crate::ast::statement::BindingResidency;
use crate::ast::types::{FunctionTypeData, STRING_TYPE_NAME};
use crate::ast::BuiltinCollectionKind as Collection;
use crate::ast::{ExpressionKind, TypeKind};

/// The token [`type_kind_to_mangle_str`] yields for a closed type it cannot
/// name: one nested past [`MAX_TOKEN_DEPTH`], or a closure type declaring type
/// parameters of its own or taking a gpu-resident parameter.
///
/// Such a type has no per-instantiation symbol, and every one of them spells
/// alike, so a symbol carrying this token names no single instantiation and
/// is never claimed for a body. Every caller that decides whether a body can
/// be named asks [`crate::mir::lowering::has_a_monomorphized_spelling`], which
/// compares against this token and [`OPEN_PARAMETER_TOKEN`] — so the spelling
/// and the decision can never drift apart. The leading underscores keep it out
/// of reach of a user type that spells itself the same way, which would
/// otherwise lose its own body to this answer.
pub(crate) const UNSPELLABLE_TYPE_TOKEN: &str = "__unspellable";

/// The token [`type_kind_to_mangle_str`] yields for a type that still names a
/// type parameter no substitution has bound, or for a value argument that is
/// still an unfolded expression: not an instantiation yet, but the body shared
/// by every instantiation of its declaration, which a body lowered without a
/// substitution calls.
pub(crate) const OPEN_PARAMETER_TOKEN: &str = "__open";

/// How deep [`type_kind_to_mangle_str`] descends into a type's components
/// before giving up on naming it.
///
/// A type nested past this is spelled [`UNSPELLABLE_TYPE_TOKEN`], and every
/// such type spells it alike, so it cannot be told apart from any other in a
/// symbol. An instance of a generic class, a call to a generic function and
/// a `gpu fn` launch at one are refused where they are lowered, and a symbol built from the
/// token is never claimed for a body or given a drop function: two
/// instantiations must not meet in one symbol and share a body laid out for
/// only one of them. Bounding the descent keeps a deeply nested type in a
/// source file from being a way to exhaust the compiler's stack.
pub(crate) const MAX_TOKEN_DEPTH: usize = 64;

/// The token one type argument contributes to a mangled name.
///
/// A built-in kind spells itself. A user-defined type spells its own name, so
/// two instantiations of the same generic at two different classes get two
/// symbols: sharing one would make the second instantiation run the first one's
/// body against its own field layout.
///
/// A type built out of others — an instantiated generic class, an optional, a
/// tuple — spells its own token followed by its components', because those
/// components are what a body compiled for it addresses. A `List<String>`
/// element and a `List<int>` element that shared the token `List` would share a
/// body, and the one holding strings would then be filled without taking a
/// reference to any of them.
///
/// Two different types never share a token. The token is a run of segments
/// joined by `_`, and each segment says what it is by its first character:
///
/// - A segment that starts with a letter is a leaf or the head of a declared
///   generic: a primitive's spelling, or a declared type's name when that name
///   is a letter followed by letters and digits (see [`is_plain_type_name`]). A
///   declared head takes the fixed number of arguments its declaration gives it.
/// - A segment made only of digits (after an optional `-`) is a value generic's
///   constant.
/// - A segment of digits followed by letters is structure no identifier can
///   spell, since an identifier never starts with a digit. The digits count
///   what follows. `<n>tuple`, `<n>fn`, `1option`, `2result`, `1future` and
///   `1out` are built-in heads taking `n` component tokens (a closure's `n`
///   counts its parameters; its return follows them); `<n>x` is the declared
///   name of the next `n` characters, spelled that way when it is not plain.
///
/// Two different types never share a token, except the types it has none for:
/// a component that has no token of its own makes the whole type nameless —
/// open when a component names an unbound parameter, unspellable otherwise —
/// since a name built from either would be the same name for every type that
/// contains one.
pub(crate) fn type_kind_to_mangle_str(kind: &TypeKind) -> Cow<'static, str> {
    type_kind_token(kind, 0)
}

/// [`type_kind_to_mangle_str`] one level into a type, carrying how far the
/// descent has already gone.
fn type_kind_token(kind: &TypeKind, depth: usize) -> Cow<'static, str> {
    if depth >= MAX_TOKEN_DEPTH {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    if let Some(value_expr) = crate::type_checker::generics::extract_value_generic_kind(kind) {
        return expression_token(value_expr, depth + 1);
    }
    let token: &'static str = match kind {
        TypeKind::Int => "int",
        TypeKind::Float | TypeKind::F64 => "float",
        TypeKind::F32 => "f32",
        TypeKind::F16 => "f16",
        TypeKind::Boolean => "bool",
        TypeKind::String => STRING_TYPE_NAME,
        TypeKind::Void => "void",
        TypeKind::Custom(name, None) => return declared_name_token(name),
        TypeKind::Custom(name, Some(args)) => {
            return compound_token(declared_name_token(name), args, depth)
        }
        TypeKind::List(inner) => return collection_token(Collection::List, [&**inner], depth),
        TypeKind::Set(inner) => return collection_token(Collection::Set, [&**inner], depth),
        TypeKind::Array(inner, size) => {
            return collection_token(Collection::Array, [&**inner, &**size], depth)
        }
        TypeKind::Map(key, value) => {
            return collection_token(Collection::Map, [&**key, &**value], depth)
        }
        TypeKind::Option(inner) => {
            return join_tokens(
                built_in_head(OPTION_HEAD, 1),
                [type_kind_token(&inner.kind, depth + 1)].into_iter(),
            )
        }
        TypeKind::Tuple(elements) => {
            return compound_token(built_in_head(TUPLE_HEAD, elements.len()), elements, depth)
        }
        TypeKind::Result(ok, err) => {
            return compound_token(built_in_head(RESULT_HEAD, 2), [&**ok, &**err], depth)
        }
        TypeKind::Future(inner) => {
            return compound_token(built_in_head(FUTURE_HEAD, 1), [&**inner], depth)
        }
        TypeKind::Function(function) => return closure_token(function, depth),
        TypeKind::I8 => "i8",
        TypeKind::I16 => "i16",
        TypeKind::I32 => "i32",
        TypeKind::I64 => "i64",
        TypeKind::I128 => "i128",
        TypeKind::U8 => "u8",
        TypeKind::U16 => "u16",
        TypeKind::U32 => "u32",
        TypeKind::U64 => "u64",
        TypeKind::U128 => "u128",
        TypeKind::RawPtr => "RawPtr",
        TypeKind::Generic(_, _, _) => OPEN_PARAMETER_TOKEN,
        TypeKind::Meta(_) | TypeKind::Linear(_) | TypeKind::Identifier | TypeKind::Error => {
            UNSPELLABLE_TYPE_TOKEN
        }
    };
    Cow::Borrowed(token)
}

/// The segment heading a built-in type of `arity` components: the arity, then
/// `tag`. Starting with a digit is what keeps it out of reach of any declared
/// name, and the arity is what tells a decoder where the components end.
fn built_in_head(tag: &str, arity: usize) -> Cow<'static, str> {
    Cow::Owned(format!("{arity}{tag}"))
}

/// Whether a declared type's `name` can spell itself as is: a letter followed
/// by letters and digits, and not a built-in leaf's spelling. Such a name is one
/// segment on its own and cannot be mistaken for anything a built-in spells.
fn is_plain_type_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic())
        && chars.all(|c| c.is_ascii_alphanumeric())
        && name != VOID_TOKEN
}

/// The token of a declared type's name: the name itself when it is plain (see
/// [`is_plain_type_name`]), otherwise its length, [`ESCAPED_NAME_TAG`] and the
/// name — `My_Box` is `6xMy_Box` — so an underscore inside it can never be read
/// as the boundary between two components.
fn declared_name_token(name: &str) -> Cow<'static, str> {
    if is_plain_type_name(name) {
        return Cow::Owned(name.to_string());
    }
    Cow::Owned(format!("{}{ESCAPED_NAME_TAG}{name}", name.len()))
}

/// The token for a built-in collection: its class's name followed by its
/// components, the same token a reference to that class spells.
fn collection_token<'a>(
    collection: Collection,
    args: impl IntoIterator<Item = &'a Expression>,
    depth: usize,
) -> Cow<'static, str> {
    compound_token(declared_name_token(collection.name()), args, depth)
}

/// The token for a closure type: a head carrying its arity, each parameter's
/// token — wrapped in a `1out` head for one the closure writes through — and
/// the return's (`void` when it returns nothing): `fn(x int) String` is
/// `1fn_int_String`.
///
/// A closure type declaring type parameters of its own, or taking a parameter
/// resident anywhere but the host, has no token: it crosses a call in a way no
/// instantiation of a class can hold.
fn closure_token(function: &FunctionTypeData, depth: usize) -> Cow<'static, str> {
    let on_host = |param: &crate::ast::common::Parameter| match param.residency {
        None | Some(BindingResidency::Host) => true,
        Some(BindingResidency::Gpu) => false,
    };
    if function.generics.is_some() || !function.params.iter().all(on_host) {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    let params = function.params.iter().flat_map(|param| {
        let written = expression_token(&param.typ, depth + 1);
        let marker = param.is_out.then(|| built_in_head(OUT_PARAMETER_HEAD, 1));
        marker.into_iter().chain(std::iter::once(written))
    });
    let ret = match function.return_type.as_deref() {
        Some(ret) => expression_token(ret, depth + 1),
        None => Cow::Borrowed(VOID_TOKEN),
    };
    join_tokens(
        built_in_head(CLOSURE_HEAD, function.params.len()),
        params.chain(std::iter::once(ret)),
    )
}

/// The token for a type written as `head` applied to `args`, e.g. `Map<String,
/// int>` → `Map_String_int`.
fn compound_token<'a>(
    head: Cow<'static, str>,
    args: impl IntoIterator<Item = &'a Expression>,
    depth: usize,
) -> Cow<'static, str> {
    join_tokens(
        head,
        args.into_iter().map(|arg| expression_token(arg, depth + 1)),
    )
}

/// Join `head` and each component token with `_`, collapsing to a nameless
/// token when a component has no token of its own: [`OPEN_PARAMETER_TOKEN`]
/// when any component is still open, as the type is then not an instantiation
/// yet, otherwise [`UNSPELLABLE_TYPE_TOKEN`].
fn join_tokens(
    head: Cow<'static, str>,
    parts: impl Iterator<Item = Cow<'static, str>>,
) -> Cow<'static, str> {
    let mut out = head.into_owned();
    let mut is_unspellable = false;
    for part in parts {
        if part == OPEN_PARAMETER_TOKEN {
            return Cow::Borrowed(OPEN_PARAMETER_TOKEN);
        }
        is_unspellable |= part == UNSPELLABLE_TYPE_TOKEN;
        if !is_unspellable {
            out.push('_');
            out.push_str(&part);
        }
    }
    if is_unspellable {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    Cow::Owned(out)
}

/// Whether `token` names no type: an open one or one that cannot be named.
pub(crate) fn is_nameless(token: &str) -> bool {
    token == UNSPELLABLE_TYPE_TOKEN || token == OPEN_PARAMETER_TOKEN
}

/// The tag of the built-in head spelling a tuple of its components.
const TUPLE_HEAD: &str = "tuple";
/// The tag of the built-in head spelling a closure type.
const CLOSURE_HEAD: &str = "fn";
/// The tag of the built-in head spelling an optional of its payload.
const OPTION_HEAD: &str = "option";
/// The tag of the built-in head spelling a result of its two payloads.
const RESULT_HEAD: &str = "result";
/// The tag of the built-in head spelling a future of its payload.
const FUTURE_HEAD: &str = "future";
/// The tag of the built-in head wrapping a closure parameter written through.
const OUT_PARAMETER_HEAD: &str = "out";
/// The tag spelling a declared name that is not plain, after its length.
const ESCAPED_NAME_TAG: &str = "x";
/// The spelling of the empty type, the one built-in leaf spelling a declared
/// type can take for its own name — every other one is a primitive's name,
/// which resolves to the primitive before any declaration is consulted.
const VOID_TOKEN: &str = "void";

/// The token a generic argument expression contributes to a mangled name.
///
/// A type argument spells its type. A value generic — the `3` in `Array<T, 3>`
/// — spells the constant itself, so two sizes of the same element type get two
/// symbols instead of sharing one body between them. A size that is still an
/// unfolded expression names no single instantiation and is open.
fn expression_mangle_token(arg: &Expression) -> Cow<'static, str> {
    expression_token(arg, 0)
}

/// [`expression_mangle_token`] one level into a type, carrying how far the
/// descent has already gone.
fn expression_token(arg: &Expression, depth: usize) -> Cow<'static, str> {
    if depth >= MAX_TOKEN_DEPTH {
        return Cow::Borrowed(UNSPELLABLE_TYPE_TOKEN);
    }
    match &arg.node {
        ExpressionKind::Type(ty, _) => type_kind_token(&ty.kind, depth),
        ExpressionKind::Literal(crate::ast::literal::Literal::Integer(value)) => {
            Cow::Owned(value.to_string())
        }
        _ => Cow::Borrowed(OPEN_PARAMETER_TOKEN),
    }
}

/// Whether the symbol mangler has a token for `kind`. See
/// [`UNSPELLABLE_TYPE_TOKEN`] and [`OPEN_PARAMETER_TOKEN`].
pub(crate) fn kind_has_a_mangled_token(kind: &TypeKind) -> bool {
    !is_nameless(&type_kind_to_mangle_str(kind))
}

/// Whether the symbol mangler has a token for the generic argument `arg`. See
/// [`expression_mangle_token`].
pub(crate) fn argument_has_a_mangled_token(arg: &Expression) -> bool {
    !is_nameless(&expression_mangle_token(arg))
}

#[cfg(test)]
mod mangled_token_tests {
    use super::*;
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::IdNode;
    use crate::ast::Type;
    use crate::error::syntax::Span;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, span())
    }

    fn type_arg(kind: TypeKind) -> Expression {
        IdNode::new(0, ExpressionKind::Type(Box::new(ty(kind)), false), span())
    }

    fn size_arg(value: i64) -> Expression {
        IdNode::new(
            0,
            ExpressionKind::Literal(Literal::Integer(IntegerLiteral::I64(value))),
            span(),
        )
    }

    fn class_ref(name: &str, args: Vec<Expression>) -> TypeKind {
        TypeKind::Custom(name.to_string(), Some(args))
    }

    fn token(kind: TypeKind) -> String {
        type_kind_to_mangle_str(&kind).into_owned()
    }

    /// The size decides which body a call reaches, so two sizes of one element
    /// type must not share a symbol.
    #[test]
    fn two_array_sizes_mangle_to_two_symbols() {
        let three = class_ref("Array", vec![type_arg(TypeKind::String), size_arg(3)]);
        let four = class_ref("Array", vec![type_arg(TypeKind::String), size_arg(4)]);
        assert_eq!(token(three.clone()), "Array_String_3");
        assert_ne!(token(three), token(four));
    }

    /// A value generic reaches the mangler through the instantiation registry as
    /// a marker type; it must spell the same constant the class reference does.
    #[test]
    fn a_value_generic_marker_spells_its_constant() {
        let marker = crate::type_checker::generics::value_generic_marker_type(size_arg(3));
        assert_eq!(token(marker.kind), "3");
    }

    /// Two lists that differ only in their element are two different types, and
    /// a body compiled for one reads the other's elements at the wrong
    /// ownership.
    #[test]
    fn two_list_elements_mangle_to_two_symbols() {
        let ints = class_ref("List", vec![type_arg(TypeKind::Int)]);
        let strings = class_ref("List", vec![type_arg(TypeKind::String)]);
        assert_eq!(token(ints.clone()), "List_int");
        assert_ne!(token(ints), token(strings));
    }

    #[test]
    fn an_optional_spells_its_payload() {
        let strings = TypeKind::Option(Box::new(ty(TypeKind::String)));
        let ints = TypeKind::Option(Box::new(ty(TypeKind::Int)));
        assert_eq!(token(strings.clone()), "1option_String");
        assert_ne!(token(strings), token(ints));
    }

    #[test]
    fn a_tuple_spells_every_component() {
        let pair = TypeKind::Tuple(vec![type_arg(TypeKind::Int), type_arg(TypeKind::String)]);
        assert_eq!(token(pair), "2tuple_int_String");
    }

    fn closure(params: Vec<TypeKind>, ret: Option<TypeKind>) -> TypeKind {
        let params = params
            .into_iter()
            .enumerate()
            .map(|(index, kind)| crate::ast::common::Parameter {
                name: format!("p{index}"),
                name_span: span(),
                typ: Box::new(type_arg(kind)),
                guard: None,
                default_value: None,
                is_out: false,
                residency: None,
            })
            .collect();
        TypeKind::Function(Box::new(FunctionTypeData {
            generics: None,
            params,
            return_type: ret.map(|kind| Box::new(type_arg(kind))),
        }))
    }

    /// A closure type spells its arity, each parameter and its return, so two
    /// closure shapes never share a body.
    #[test]
    fn a_closure_type_spells_its_arity_parameters_and_return() {
        let thunk = closure(Vec::new(), Some(TypeKind::String));
        let step = closure(vec![TypeKind::Int], Some(TypeKind::Int));
        let action = closure(vec![TypeKind::Int], None);
        assert_eq!(token(thunk), "0fn_String");
        assert_eq!(token(step.clone()), "1fn_int_int");
        assert_eq!(token(action.clone()), "1fn_int_void");
        assert_ne!(token(step), token(action));
    }

    /// A closure taking an `out` parameter crosses the call by address, so it
    /// never shares a token with one taking the same type by value.
    #[test]
    fn a_closure_with_an_out_parameter_spells_it() {
        let reads = closure(vec![TypeKind::Int], None);
        let mut writes = reads.clone();
        if let TypeKind::Function(function) = &mut writes {
            function.params[0].is_out = true;
        }
        assert_eq!(token(writes.clone()), "1fn_1out_int_void");
        assert_ne!(token(writes), token(reads));
    }

    /// A closure declaring type parameters of its own names no single
    /// instantiation.
    #[test]
    fn a_closure_with_type_parameters_has_no_token() {
        let mut generic = closure(vec![TypeKind::Int], None);
        if let TypeKind::Function(function) = &mut generic {
            function.generics = Some(Vec::new());
        }
        assert!(!kind_has_a_mangled_token(&generic));
    }

    #[test]
    fn a_result_spells_both_payloads() {
        let result = TypeKind::Result(
            Box::new(type_arg(TypeKind::String)),
            Box::new(type_arg(TypeKind::Int)),
        );
        assert_eq!(token(result), "2result_String_int");
    }

    #[test]
    fn a_future_spells_its_payload() {
        let future = TypeKind::Future(Box::new(type_arg(TypeKind::String)));
        assert_eq!(token(future), "1future_String");
    }

    fn named(name: &str) -> TypeKind {
        TypeKind::Custom(name.to_string(), None)
    }

    fn tuple(elements: Vec<TypeKind>) -> TypeKind {
        TypeKind::Tuple(elements.into_iter().map(type_arg).collect())
    }

    fn with_out_parameter(mut closure_type: TypeKind) -> TypeKind {
        if let TypeKind::Function(function) = &mut closure_type {
            function.params[0].is_out = true;
        }
        closure_type
    }

    fn int_pair() -> TypeKind {
        tuple(vec![TypeKind::Int, TypeKind::Int])
    }

    fn step() -> TypeKind {
        closure(vec![TypeKind::Int], Some(TypeKind::Int))
    }

    /// Pairs of a declared type and a built-in one a token writer spelling
    /// built-in heads the way an identifier can would spell alike.
    fn declared_name_pairs() -> Vec<(&'static str, TypeKind, TypeKind)> {
        vec![
            (
                "a declared generic named like a closure head",
                class_ref(
                    "fn1",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                step(),
            ),
            (
                "a declared generic named like a tuple head",
                class_ref(
                    "tuple",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                int_pair(),
            ),
            (
                "a declared generic named like an optional head",
                class_ref("option", vec![type_arg(TypeKind::Int)]),
                TypeKind::Option(Box::new(ty(TypeKind::Int))),
            ),
            (
                "a declared generic named like a result head",
                class_ref(
                    "result",
                    vec![type_arg(TypeKind::Int), type_arg(TypeKind::Int)],
                ),
                TypeKind::Result(
                    Box::new(type_arg(TypeKind::Int)),
                    Box::new(type_arg(TypeKind::Int)),
                ),
            ),
            (
                "a declared generic named like a future head",
                class_ref("future", vec![type_arg(TypeKind::Int)]),
                TypeKind::Future(Box::new(type_arg(TypeKind::Int))),
            ),
            (
                "a declared type named like the empty type",
                named("void"),
                TypeKind::Void,
            ),
            (
                "a declared type named like the unspellable token",
                named(UNSPELLABLE_TYPE_TOKEN),
                TypeKind::Generic(
                    "T".to_string(),
                    None,
                    crate::ast::types::TypeDeclarationKind::None,
                ),
            ),
            (
                "a declared name holding an underscore",
                named("Box_int"),
                class_ref("Box", vec![type_arg(TypeKind::Int)]),
            ),
        ]
    }

    /// Pairs of built-in types a token writer leaving an arity implicit would
    /// spell alike.
    fn structural_pairs() -> Vec<(&'static str, TypeKind, TypeKind)> {
        vec![
            (
                "nested tuples of equal flattened leaves",
                tuple(vec![int_pair(), TypeKind::String, TypeKind::Int]),
                tuple(vec![
                    tuple(vec![TypeKind::Int, TypeKind::Int, TypeKind::String]),
                    TypeKind::Int,
                ]),
            ),
            (
                "a tuple nested first or last",
                tuple(vec![int_pair(), TypeKind::Int]),
                tuple(vec![TypeKind::Int, int_pair()]),
            ),
            (
                "closures of one arity that return a tuple or take one",
                closure(vec![int_pair()], Some(TypeKind::Int)),
                closure(vec![TypeKind::Int], Some(int_pair())),
            ),
            (
                "a closure taking an out parameter or not",
                with_out_parameter(step()),
                step(),
            ),
            (
                "an out parameter or a declared type named like its head",
                with_out_parameter(step()),
                closure(vec![named("out"), TypeKind::Int], Some(TypeKind::Int)),
            ),
        ]
    }

    /// Two different types never share a token, whatever a declaration names
    /// itself and however the types nest.
    #[test]
    fn distinct_types_spell_distinct_tokens() {
        for (case, left, right) in declared_name_pairs().into_iter().chain(structural_pairs()) {
            let (left, right) = (token(left), token(right));
            assert_ne!(left, right, "{case}: both spell `{left}`");
        }
    }

    /// A plain declared name spells itself; one an identifier-shaped built-in
    /// could collide with carries its length.
    #[test]
    fn a_declared_name_that_is_not_plain_carries_its_length() {
        assert_eq!(token(named("Point")), "Point");
        assert_eq!(token(named("Box_int")), "7xBox_int");
        assert_eq!(token(named("void")), "4xvoid");
    }

    /// A value argument past `i128` spells the value written, not the one it
    /// wraps to.
    #[test]
    fn a_value_past_i128_spells_the_value_written() {
        let big = IdNode::new(
            0,
            ExpressionKind::Literal(Literal::Integer(IntegerLiteral::U128(1 << 127))),
            span(),
        );
        assert_eq!(
            expression_mangle_token(&big),
            "170141183460469231731687303715884105728"
        );
    }

    /// A component with no token makes the whole type unspellable: a name
    /// assembled from the unspellable token would be one name for every such
    /// type.
    #[test]
    fn a_component_without_a_token_makes_the_whole_type_unspellable() {
        let open = TypeKind::Generic(
            "T".to_string(),
            None,
            crate::ast::types::TypeDeclarationKind::None,
        );
        assert!(!kind_has_a_mangled_token(&open));
        assert!(!kind_has_a_mangled_token(&class_ref(
            "List",
            vec![type_arg(open)]
        )));
    }

    /// The verifier stays quiet about a receiver no per-instantiation body
    /// could be named for, so what it exempts is exactly what this predicate
    /// declines. A sized array and a list of structural elements are both
    /// nameable now, which is what puts them back inside the check.
    #[test]
    fn a_sized_array_and_a_structural_element_can_both_be_named() {
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "Array",
            vec![type_arg(TypeKind::String), size_arg(3)]
        )));
        let pair = TypeKind::Tuple(vec![type_arg(TypeKind::Int), type_arg(TypeKind::String)]);
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "List",
            vec![type_arg(pair)]
        )));
        assert!(crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "List",
            vec![type_arg(class_ref("List", vec![type_arg(TypeKind::Int)]))]
        )));
    }

    /// A size still written as an expression names no single instantiation, so
    /// the receiver holding it gets no per-instantiation body.
    #[test]
    fn an_unfolded_size_has_no_token() {
        let unfolded = IdNode::new(0, ExpressionKind::Identifier("N".to_string(), None), span());
        assert!(!argument_has_a_mangled_token(&unfolded));
        assert!(!crate::mir::lowering::can_be_monomorphized_at(&class_ref(
            "Array",
            vec![type_arg(TypeKind::String), unfolded]
        )));
    }

    fn open_parameter() -> TypeKind {
        TypeKind::Generic(
            "T".to_string(),
            None,
            crate::ast::types::TypeDeclarationKind::None,
        )
    }

    fn nested_past_the_depth_bound(leaf: TypeKind) -> TypeKind {
        (0..MAX_TOKEN_DEPTH + 1).fold(leaf, |inner, _| TypeKind::Option(Box::new(ty(inner))))
    }

    /// A type nested past the depth bound cannot be named, and every such
    /// type spells alike.
    #[test]
    fn a_type_nested_past_the_depth_bound_is_unspellable() {
        assert_eq!(
            token(nested_past_the_depth_bound(TypeKind::Int)),
            UNSPELLABLE_TYPE_TOKEN
        );
        assert_eq!(
            token(nested_past_the_depth_bound(TypeKind::String)),
            UNSPELLABLE_TYPE_TOKEN
        );
    }

    /// An unbound type parameter is open rather than unspellable, and a type
    /// naming one anywhere is open even beside a component with no name.
    #[test]
    fn a_type_naming_an_unbound_parameter_is_open() {
        assert_eq!(token(open_parameter()), OPEN_PARAMETER_TOKEN);
        let beside_an_unspellable = tuple(vec![
            nested_past_the_depth_bound(TypeKind::Int),
            open_parameter(),
        ]);
        assert_eq!(token(beside_an_unspellable), OPEN_PARAMETER_TOKEN);
        assert!(!kind_has_a_mangled_token(&open_parameter()));
    }
}
