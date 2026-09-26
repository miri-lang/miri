// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Where the body of a method a generic class inherits is compiled.
//!
//! A class that inherits a method gets no copy of it: the body belongs to the
//! ancestor that declares it and is monomorphized once per instantiation of
//! *that* ancestor. So a call on `Child<String>` to an `equals` declared by
//! `Base` reaches `Base`'s body at `Base`'s own type arguments, which the
//! `extends` clause maps from the child's.
//!
//! Every place that names such a body — the MIR call site, the operator
//! callee, the container's element thunk, and the pipeline that decides which
//! instantiation bodies to lower — asks here, so they cannot drift onto
//! different symbols for one body.

use crate::ast::types::Type;
use crate::type_checker::context::TypeDefinition;

use std::collections::HashMap;

/// One class, instantiated at concrete type arguments.
pub(crate) type ClassInstantiation = (String, Vec<Type>);

/// The class declaring `method_name` for `class_name`, and the type arguments
/// that class is instantiated at when `class_name` carries `type_args`.
///
/// Returns `class_name` and `type_args` unchanged when the class declares the
/// method itself. `None` when no class in the chain declares it, or when a
/// link's `extends` arguments do not fill the parent's generic parameters —
/// there is no instantiation to name in either case.
pub(crate) fn declaring_class_instantiation(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
    type_args: &[Type],
    method_name: &str,
) -> Option<ClassInstantiation> {
    let mut current = class_name.to_string();
    let mut current_args = type_args.to_vec();

    // A circular `extends` is reported where the class is declared; bounding
    // the walk by the number of definitions keeps this from hanging before the
    // report is produced.
    for _ in 0..type_definitions.len() {
        let Some(TypeDefinition::Class(class_def)) = type_definitions.get(&current) else {
            return None;
        };
        if compiles_its_own_body(class_def, method_name, type_definitions) {
            return Some((current, current_args));
        }
        let (base, base_args) =
            base_class_instantiation(type_definitions, &current, &current_args)?;
        current = base;
        current_args = base_args;
    }
    None
}

/// The class `class_name` extends and the type arguments it is instantiated at
/// when `class_name` carries `type_args`.
///
/// The `extends Base<...>` arguments are written in the child's generic-param
/// scope, so substituting the child's own arguments into them yields the
/// parent's: `Child<U> extends Base<List<U>>` at `U = String` gives
/// `List<String>`. `None` when the class extends nothing, extends something
/// that is not a generic class, or carries arguments its own parameters do not
/// account for — none of those name an instantiation of a parent.
pub(crate) fn base_class_instantiation(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
    type_args: &[Type],
) -> Option<ClassInstantiation> {
    let Some(TypeDefinition::Class(class_def)) = type_definitions.get(class_name) else {
        return None;
    };
    let base = class_def.base_class.as_deref()?;
    if !class_takes_generics(base, type_definitions) {
        return None;
    }
    let declared = class_def.base_class_args.as_ref()?;
    let subs = class_substitution(class_def, type_args)?;
    let base_args = declared
        .iter()
        .map(|arg| super::apply_generic_sub(arg, &subs))
        .collect();
    Some((base.to_string(), base_args))
}

/// The type each field of an instance of `class_name` at `type_args` is stored
/// at, in the order [`collect_class_fields_all`] lists them.
///
/// A field is written in the parameters of the class that *declares* it, and a
/// class reaches its parent's parameters through its own `extends` clause. So
/// each ancestor's fields are substituted by the arguments that ancestor is
/// reached at — `class Child extends Base<String>` stores `Base`'s `value T` as
/// a `String` although the child carries no parameter — and never by the
/// arguments of the class furthest down, whose parameters may share a name with
/// the parent's and mean something else entirely.
///
/// [`collect_class_fields_all`]: crate::type_checker::context::collect_class_fields_all
// TODO: a generic class inheriting from another loses one inherited field when
// a reference-counted field is among them. `class Child<X, Y, Z> extends
// Base<X, Y, Z>` holding a `float`, an `int` and a `String` writes `first`
// somewhere other than where it is read back, so it returns whatever the
// allocator left there. Two fields are fine, and three are fine while all of
// them are scalars, which points at the field kind rather than the count. It
// only shows reliably under `MIRI_HEAP_GUARD=1`, whose own allocations change
// what the stale memory holds; without the guard the leftover bytes happen to
// be the right answer, so a test written without it proves nothing.
pub(crate) fn instantiated_field_types(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
    type_args: &[Type],
) -> Vec<Type> {
    ancestry_instantiations(type_definitions, class_name, type_args)
        .iter()
        .rev()
        .flat_map(|(class_def, args)| {
            let subs = class_substitution(class_def, args).unwrap_or_default();
            class_def
                .fields
                .iter()
                .map(move |(_, field)| super::apply_generic_sub(&field.ty, &subs))
        })
        .collect()
}

/// The type each field of `class_name` is declared at, written in
/// `class_name`'s **own** type parameters, in the order
/// [`collect_class_fields_all`] lists them.
///
/// An ancestor declares its fields in its own parameters, and the `extends`
/// clause says what those are bound to. A caller holding no concrete arguments
/// — the shared body of a generic class, or the table a reference-counting pass
/// reads a field projection through — still has to follow that binding: in
/// `class Child<X, Y> extends Base<Y, X>` the inherited `left`, declared `A`,
/// is a `Y`, and reading it as `A` names a parameter the child does not have.
///
/// [`collect_class_fields_all`]: crate::type_checker::context::collect_class_fields_all
pub(crate) fn declared_field_types(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<Type> {
    let own_args = own_parameters_as_arguments(type_definitions, class_name);
    instantiated_field_types(type_definitions, class_name, &own_args)
}

/// `class_name`'s own type parameters, spelled as the type arguments an
/// instance of it at itself would carry. Empty for a class declaring none.
pub(crate) fn own_parameters_as_arguments(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
) -> Vec<Type> {
    own_parameters(type_definitions, class_name)
        .iter()
        .map(open_parameter)
        .collect()
}

/// `class_name`'s own type parameters, each bound to itself as an open
/// parameter. Empty for a class declaring none.
///
/// A generic class's bare copy of an inherited default reads the trait's
/// parameters through the class's clauses: `class Base<U> implements Op<U>`
/// binds the trait's `T` to the class's open `U`. Left unbound instead, `T`
/// names no parameter of the class, and a local declared at it is lowered as a
/// value of an unknown named type that owns a reference.
pub(crate) fn own_parameters_left_open(
    type_definitions: &HashMap<String, TypeDefinition>,
    class_name: &str,
) -> HashMap<String, Type> {
    own_parameters(type_definitions, class_name)
        .iter()
        .map(|param| (param.name.clone(), open_parameter(param)))
        .collect()
}

/// The type parameters `class_name` declares, none for a class declaring none.
fn own_parameters<'td>(
    type_definitions: &'td HashMap<String, TypeDefinition>,
    class_name: &str,
) -> &'td [crate::type_checker::context::GenericDefinition] {
    match type_definitions.get(class_name) {
        Some(TypeDefinition::Class(class_def)) => class_def.generics.as_deref().unwrap_or_default(),
        Some(
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Trait(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_),
        )
        | None => &[],
    }
}

/// `param` as the open type it stands for inside its class, carrying the
/// bound it is declared with.
fn open_parameter(param: &crate::type_checker::context::GenericDefinition) -> Type {
    let bound = param.constraint.clone().map(Box::new);
    Type::new(
        crate::ast::types::TypeKind::Generic(param.name.clone(), bound, param.kind),
        crate::error::syntax::Span::new(0, 0),
    )
}

/// Whether a body for `method_name` is compiled under `class_def`'s own name.
///
/// That holds for a method the class declares, and equally for one a trait it
/// lists supplies a default for: a default is re-lowered once per implementing
/// class, under that class's symbol, so the class owns that body as much as a
/// written method. Only a method reached through `extends` belongs elsewhere.
fn compiles_its_own_body(
    class_def: &crate::type_checker::context::ClassDefinition,
    method_name: &str,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    class_def.methods.contains_key(method_name)
        || class_def.traits.iter().any(|trait_name| {
            crate::type_checker::context::find_trait_default_method(
                type_definitions,
                trait_name,
                method_name,
            )
            .is_some()
        })
}

/// `class_name` and every class it extends, nearest first, each paired with the
/// type arguments it is reached at.
///
/// The walk visits exactly the classes [`collect_class_fields_all`] does, so a
/// caller can pair the two results field by field. A circular `extends` is
/// reported where the class is declared; bounding the walk by the number of
/// definitions keeps this from hanging before that report is produced.
///
/// [`collect_class_fields_all`]: crate::type_checker::context::collect_class_fields_all
fn ancestry_instantiations<'a>(
    type_definitions: &'a HashMap<String, TypeDefinition>,
    class_name: &str,
    type_args: &[Type],
) -> Vec<(&'a crate::type_checker::context::ClassDefinition, Vec<Type>)> {
    let mut chain = Vec::new();
    let mut next = Some((class_name.to_string(), type_args.to_vec()));
    for _ in 0..=type_definitions.len() {
        let Some((current, current_args)) = next.take() else {
            break;
        };
        let Some(TypeDefinition::Class(class_def)) = type_definitions.get(&current) else {
            break;
        };
        // An ancestor whose arguments the `extends` chain does not pin — a
        // non-generic parent, or one reached through a clause that fills none
        // of its parameters — is reached at no arguments, and its fields keep
        // the spelling they were declared with.
        next = class_def.base_class.as_ref().map(|base| {
            base_class_instantiation(type_definitions, &current, &current_args)
                .unwrap_or_else(|| (base.clone(), Vec::new()))
        });
        chain.push((class_def, current_args));
    }
    chain
}

/// Whether `class_name` declares generic parameters.
fn class_takes_generics(
    class_name: &str,
    type_definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    matches!(
        type_definitions.get(class_name),
        Some(TypeDefinition::Class(def)) if def.generics.is_some()
    )
}

/// `class_def`'s generic parameters paired with `class_args`, or `None` when
/// the counts disagree and the pairing would be a guess.
///
/// A class that declares no parameters substitutes nothing: it is reached at no
/// arguments, and whatever its `extends` clause writes is already concrete.
fn class_substitution(
    class_def: &crate::type_checker::context::ClassDefinition,
    class_args: &[Type],
) -> Option<HashMap<String, Type>> {
    let Some(generics) = class_def.generics.as_ref() else {
        return class_args.is_empty().then(HashMap::new);
    };
    if generics.len() != class_args.len() {
        return None;
    }
    Some(
        generics
            .iter()
            .zip(class_args)
            .map(|(g, t)| (g.name.clone(), t.clone()))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::types::{TypeDeclarationKind, TypeKind};
    use crate::ast::MemberVisibility;
    use crate::error::syntax::Span;
    use crate::type_checker::context::{ClassDefinition, GenericDefinition, MethodInfo};

    use std::collections::BTreeMap;

    fn span() -> Span {
        Span::new(0, 0)
    }

    fn generic_param(name: &str) -> GenericDefinition {
        GenericDefinition {
            name: name.to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }
    }

    fn generic_type(name: &str) -> Type {
        Type::new(
            TypeKind::Generic(name.to_string(), None, TypeDeclarationKind::None),
            span(),
        )
    }

    fn string_type() -> Type {
        Type::new(TypeKind::String, span())
    }

    fn method() -> MethodInfo {
        MethodInfo {
            params: Vec::new(),
            is_out_flags: Vec::new(),
            return_type: Type::new(TypeKind::Boolean, span()),
            visibility: MemberVisibility::Public,
            is_constructor: false,
            is_abstract: false,
            is_static: false,
            attributes: Vec::new(),
        }
    }

    /// A generic class, optionally extending `base` at `base_args` and
    /// optionally declaring `equals`.
    fn class(
        name: &str,
        params: &[&str],
        base: Option<(&str, Vec<Type>)>,
        declares_equals: bool,
    ) -> ClassDefinition {
        let (base_class, base_class_args) = match base {
            Some((base_name, args)) => (Some(base_name.to_string()), Some(args)),
            None => (None, None),
        };
        let mut methods = BTreeMap::new();
        if declares_equals {
            methods.insert("equals".to_string(), method());
        }
        ClassDefinition {
            name: name.to_string(),
            generics: Some(params.iter().map(|p| generic_param(p)).collect()),
            base_class,
            base_class_args,
            traits: Vec::new(),
            trait_args: HashMap::new(),
            fields: Vec::new(),
            methods,
            module: String::new(),
            is_abstract: false,
            has_drop: false,
        }
    }

    fn definitions(classes: Vec<ClassDefinition>) -> HashMap<String, TypeDefinition> {
        classes
            .into_iter()
            .map(|def| (def.name.clone(), TypeDefinition::Class(def)))
            .collect()
    }

    #[test]
    fn a_class_declaring_the_method_is_its_own_declaring_instantiation() {
        let defs = definitions(vec![class("Base", &["T"], None, true)]);
        let found =
            declaring_class_instantiation(&defs, "Base", &[string_type()], "equals").unwrap();
        assert_eq!(found.0, "Base");
        assert_eq!(found.1[0].kind, TypeKind::String);
    }

    #[test]
    fn an_inherited_method_is_declared_by_the_parent_at_the_same_arguments() {
        let defs = definitions(vec![
            class("Base", &["T"], None, true),
            class(
                "Child",
                &["T"],
                Some(("Base", vec![generic_type("T")])),
                false,
            ),
        ]);
        let found =
            declaring_class_instantiation(&defs, "Child", &[string_type()], "equals").unwrap();
        assert_eq!(found.0, "Base");
        assert_eq!(found.1[0].kind, TypeKind::String);
    }

    #[test]
    fn parent_arguments_are_remapped_through_the_extends_clause() {
        let list_of_u = Type::new(
            TypeKind::List(Box::new(
                crate::type_checker::TypeChecker::new().create_type_expression(generic_type("U")),
            )),
            span(),
        );
        let defs = definitions(vec![
            class("Base", &["T"], None, true),
            class("Child", &["U"], Some(("Base", vec![list_of_u])), false),
        ]);
        let found =
            declaring_class_instantiation(&defs, "Child", &[string_type()], "equals").unwrap();
        assert_eq!(found.0, "Base");
        assert!(matches!(found.1[0].kind, TypeKind::List(_)));
        assert_eq!(
            crate::mir::lowering::dispatch::mangle_instantiation_name("Base_equals", &found.1),
            "Base_equals__List_String"
        );
    }

    #[test]
    fn a_swapped_extends_clause_reorders_the_parent_arguments() {
        let defs = definitions(vec![
            class("Base", &["A", "B"], None, true),
            class(
                "Child",
                &["X", "Y"],
                Some(("Base", vec![generic_type("Y"), generic_type("X")])),
                false,
            ),
        ]);
        let int = Type::new(TypeKind::Int, span());
        let found =
            declaring_class_instantiation(&defs, "Child", &[int, string_type()], "equals").unwrap();
        assert_eq!(found.0, "Base");
        assert_eq!(found.1[0].kind, TypeKind::String);
        assert_eq!(found.1[1].kind, TypeKind::Int);
    }

    #[test]
    fn a_grandparent_declaring_the_method_is_reached_through_the_chain() {
        let defs = definitions(vec![
            class("Base", &["T"], None, true),
            class(
                "Middle",
                &["T"],
                Some(("Base", vec![generic_type("T")])),
                false,
            ),
            class(
                "Leaf",
                &["T"],
                Some(("Middle", vec![generic_type("T")])),
                false,
            ),
        ]);
        let found =
            declaring_class_instantiation(&defs, "Leaf", &[string_type()], "equals").unwrap();
        assert_eq!(found.0, "Base");
        assert_eq!(found.1[0].kind, TypeKind::String);
    }

    #[test]
    fn no_class_in_the_chain_declaring_the_method_names_no_instantiation() {
        let defs = definitions(vec![
            class("Base", &["T"], None, false),
            class(
                "Child",
                &["T"],
                Some(("Base", vec![generic_type("T")])),
                false,
            ),
        ]);
        assert!(
            declaring_class_instantiation(&defs, "Child", &[string_type()], "equals").is_none()
        );
    }

    /// `class_def` with one field named `value` at `field_ty`.
    fn with_field(mut class_def: ClassDefinition, field_ty: Type) -> ClassDefinition {
        class_def.fields.push((
            "value".to_string(),
            crate::type_checker::context::FieldInfo {
                ty: field_ty,
                mutable: false,
                visibility: MemberVisibility::Public,
            },
        ));
        class_def
    }

    fn custom_type(name: &str) -> Type {
        Type::new(TypeKind::Custom(name.to_string(), None), span())
    }

    fn list_of(element: Type) -> Type {
        Type::new(
            TypeKind::List(Box::new(
                crate::type_checker::TypeChecker::new().create_type_expression(element),
            )),
            span(),
        )
    }

    #[test]
    fn a_field_resolves_to_the_argument_at_its_parameter_position() {
        let mut pair = class("Pair", &["K", "V"], None, false);
        pair = with_field(pair, generic_type("V"));
        let defs = definitions(vec![pair]);
        let int = Type::new(TypeKind::Int, span());

        let fields = instantiated_field_types(&defs, "Pair", &[string_type(), int]);

        assert_eq!(fields[0].kind, TypeKind::Int);
    }

    #[test]
    fn a_field_spelled_as_a_bare_custom_name_also_resolves() {
        // A field declared `value T` can reach lowering as `Custom("T", None)`
        // rather than `Generic("T")`; both spellings must resolve identically.
        let box_def = with_field(class("Box", &["T"], None, false), custom_type("T"));
        let defs = definitions(vec![box_def]);

        let fields = instantiated_field_types(&defs, "Box", &[string_type()]);

        assert_eq!(fields[0].kind, TypeKind::String);
    }

    #[test]
    fn an_element_type_nested_in_a_collection_field_resolves() {
        // `items [T]` is what a collection-backed generic class declares. The
        // element type has to be substituted too, or the list is dropped
        // without ever releasing what it holds.
        let box_def = with_field(
            class("Box", &["T"], None, false),
            list_of(generic_type("T")),
        );
        let defs = definitions(vec![box_def]);

        let fields = instantiated_field_types(&defs, "Box", &[string_type()]);

        let TypeKind::List(element) = &fields[0].kind else {
            panic!("expected a list field, got {:?}", fields[0].kind);
        };
        let crate::ast::expression::ExpressionKind::Type(element, _) = &element.node else {
            panic!("expected a resolved element type argument");
        };
        assert_eq!(element.kind, TypeKind::String);
    }

    #[test]
    fn a_field_naming_a_non_parameter_type_is_left_as_written() {
        let box_def = with_field(class("Box", &["T"], None, false), custom_type("Widget"));
        let defs = definitions(vec![box_def]);

        let fields = instantiated_field_types(&defs, "Box", &[string_type()]);

        assert_eq!(fields[0].kind, custom_type("Widget").kind);
    }

    #[test]
    fn a_field_reached_without_arguments_is_left_as_written() {
        // The shared bare-name drop thunk carries no arguments; the field stays
        // spelled as its parameter rather than being guessed at.
        let box_def = with_field(class("Box", &["T"], None, false), generic_type("T"));
        let defs = definitions(vec![box_def]);

        let fields = instantiated_field_types(&defs, "Box", &[]);

        assert_eq!(fields[0].kind, generic_type("T").kind);
    }

    #[test]
    fn an_inherited_field_resolves_to_what_the_extends_clause_pins() {
        let base = with_field(class("Base", &["T"], None, false), generic_type("T"));
        let mut child = class("Child", &[], Some(("Base", vec![string_type()])), false);
        child.generics = None;
        let defs = definitions(vec![base, child]);

        let fields = instantiated_field_types(&defs, "Child", &[]);

        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].kind, TypeKind::String);
    }

    #[test]
    fn an_inherited_field_is_substituted_by_the_parents_arguments_not_the_childs() {
        // Both classes name their parameter `T`, and the clause binds the
        // parent's to something else: a single flat substitution would give the
        // parent's field the child's argument.
        let base = with_field(class("Base", &["T"], None, false), generic_type("T"));
        let int = Type::new(TypeKind::Int, span());
        let child = with_field(
            class("Child", &["T"], Some(("Base", vec![int])), false),
            generic_type("T"),
        );
        let defs = definitions(vec![base, child]);

        let fields = instantiated_field_types(&defs, "Child", &[string_type()]);

        // The parent's fields come first: `collect_class_fields_all` lists the
        // root class's before the ones declared below it.
        assert_eq!(fields[0].kind, TypeKind::Int);
        assert_eq!(fields[1].kind, TypeKind::String);
    }

    #[test]
    fn a_field_of_a_non_generic_ancestor_keeps_its_own_type() {
        let mut base = class("Base", &[], None, false);
        base.generics = None;
        base = with_field(base, string_type());
        let child = with_field(
            class("Child", &["T"], Some(("Base", Vec::new())), false),
            generic_type("T"),
        );
        let defs = definitions(vec![base, child]);

        let int = Type::new(TypeKind::Int, span());
        let fields = instantiated_field_types(&defs, "Child", &[int]);

        assert_eq!(fields.len(), 2);
        assert_eq!(fields[0].kind, TypeKind::String);
        assert_eq!(fields[1].kind, TypeKind::Int);
    }

    #[test]
    fn a_non_generic_parent_names_no_instantiation() {
        let mut base = class("Base", &[], None, true);
        base.generics = None;
        let defs = definitions(vec![
            base,
            class("Child", &["T"], Some(("Base", Vec::new())), false),
        ]);
        assert!(
            declaring_class_instantiation(&defs, "Child", &[string_type()], "equals").is_none()
        );
    }
}
