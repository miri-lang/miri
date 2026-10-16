// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The instantiations a drop thunk is emitted for are every concrete one a
//! compiled body holds a value of, closed over what their fields store — not
//! only the ones whose method bodies were lowered.

use miri::ast::factory::type_expr_non_null;
use miri::ast::types::{Type, TypeDeclarationKind, TypeKind};
use miri::ast::MemberVisibility;
use miri::error::syntax::Span;
use miri::mir::body::{Body, ExecutionModel, LocalDecl};
use miri::mir::type_facts::{
    ClassDefinition, DropInstantiationRefusal, GenericDefinition, TypeDefinition, TypeFacts,
};
use miri::mir::{AggregateKind, BasicBlockData, Local, Place, Rvalue, Statement, StatementKind};
use miri::type_checker::context::FieldInfo;
use std::collections::{BTreeMap, BTreeSet, HashMap};

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, Span::default())
}

fn parameter(name: &str) -> Type {
    ty(TypeKind::Generic(
        name.to_string(),
        None,
        TypeDeclarationKind::None,
    ))
}

fn instance(name: &str, args: Vec<Type>) -> Type {
    ty(TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(type_expr_non_null).collect()),
    ))
}

/// `class {name}<T>` holding one field of type `field`.
fn generic_class(name: &str, field: Type) -> TypeDefinition {
    TypeDefinition::Class(ClassDefinition {
        name: name.to_string(),
        generics: Some(vec![GenericDefinition {
            name: "T".to_string(),
            constraint: None,
            kind: TypeDeclarationKind::None,
        }]),
        base_class: None,
        base_class_args: None,
        traits: Vec::new(),
        trait_args: HashMap::new(),
        fields: vec![(
            "value".to_string(),
            FieldInfo {
                ty: field,
                mutable: true,
                visibility: MemberVisibility::Public,
                initializer: None,
            },
        )],
        methods: BTreeMap::new(),
        module: String::new(),
        is_abstract: false,
        runtime_settled_methods: BTreeSet::new(),
    })
}

/// `Box<T>` holding a `T`, and `Outer<T>` holding a `Box<T>`.
fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::from([
        ("Box".to_string(), generic_class("Box", parameter("T"))),
        (
            "Outer".to_string(),
            generic_class("Outer", instance("Box", vec![parameter("T")])),
        ),
    ])
}

/// A body holding one local of each type in `held`.
fn body_holding(held: Vec<Type>) -> Body {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    for held_ty in held {
        body.new_local(LocalDecl::new(held_ty, Span::default()));
    }
    body
}

fn string() -> Type {
    ty(TypeKind::String)
}

#[test]
fn an_instantiation_a_body_holds_gets_a_drop_thunk_without_a_lowered_method() {
    let body = body_holding(vec![instance("Box", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_a_field_stores_is_closed_over() {
    let body = body_holding(vec![instance("Outer", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Outer", &[string()]));
    assert!(
        facts.is_drop_instantiation("Box", &[string()]),
        "the `Box<String>` an `Outer<String>` stores is released by its own thunk"
    );
}

#[test]
fn an_instantiation_nested_in_an_optional_is_found() {
    let body = body_holding(vec![ty(TypeKind::Option(Box::new(instance(
        "Box",
        vec![string()],
    ))))]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_at_an_open_parameter_gets_no_thunk() {
    let body = body_holding(vec![instance("Box", vec![parameter("T")])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.drop_instantiations_of("Box").is_empty());
}

#[test]
fn a_registered_instantiation_keeps_its_thunk() {
    let registry = HashMap::from([("Box".to_string(), vec![vec![string()]])]);
    let facts = TypeFacts::new(definitions(), registry, Default::default(), []).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
    assert!(!facts.is_drop_instantiation("Box", &[ty(TypeKind::Int)]));
}

#[test]
fn an_instantiation_only_a_closure_captures_gets_a_drop_thunk() {
    let mut body = Body::new(0, Span::default(), ExecutionModel::Cpu);
    body.closure_capture_types
        .insert(Local(0), vec![instance("Box", vec![string()])]);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn an_instantiation_only_a_construction_names_gets_a_drop_thunk() {
    // The destination is typed as something else entirely, so only the
    // construction itself names `Box<String>`.
    let mut body = body_holding(vec![ty(TypeKind::Int)]);
    let mut block = BasicBlockData::new(None);
    block.statements.push(Statement {
        kind: StatementKind::Assign(
            Place::new(Local(0)),
            Rvalue::Aggregate(
                AggregateKind::Struct(instance("Box", vec![string()])),
                Vec::new(),
            ),
        ),
        span: Span::default(),
    });
    body.basic_blocks.push(block);
    let facts = TypeFacts::new(definitions(), HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Box", &[string()]));
}

#[test]
fn a_field_nesting_its_own_type_deeper_is_refused_at_the_depth_bound() {
    // `Grow<T>` stores a `Grow<List<T>>`: every instantiation reaches a deeper
    // one, so the closure is refused once an instance passes the depth bound.
    let definitions = HashMap::from([
        ("Box".to_string(), generic_class("Box", parameter("T"))),
        (
            "Grow".to_string(),
            generic_class(
                "Grow",
                instance("Grow", vec![instance("Box", vec![parameter("T")])]),
            ),
        ),
    ]);
    let body = body_holding(vec![instance("Grow", vec![string()])]);
    let refusal = TypeFacts::new(definitions, HashMap::new(), Default::default(), [&body])
        .expect_err("an ever-deeper field has no finite set of drop thunks");

    let DropInstantiationRefusal::TooDeep { name, depth, .. } = refusal else {
        panic!("expected the depth bound to refuse, got {refusal:?}");
    };
    assert_eq!(name, "Grow");
    assert_eq!(depth, 33);
}

/// `class Duo<A, B>` holding one `value A`.
fn duo() -> TypeDefinition {
    let TypeDefinition::Class(mut def) = generic_class("Duo", parameter("A")) else {
        panic!("`generic_class` builds a class");
    };
    def.generics = Some(
        ["A", "B"]
            .iter()
            .map(|name| GenericDefinition {
                name: name.to_string(),
                constraint: None,
                kind: TypeDeclarationKind::None,
            })
            .collect(),
    );
    TypeDefinition::Class(def)
}

#[test]
fn an_unbound_argument_spelled_as_a_bare_name_names_the_same_drop_function() {
    // Inference writes an argument it left unbound either as the open
    // parameter or as its bare name; both are one instantiation.
    let bare_b = ty(TypeKind::Custom("B".to_string(), None));
    let body = body_holding(vec![instance("Duo", vec![string(), bare_b])]);
    let definitions = HashMap::from([("Duo".to_string(), duo())]);
    let facts = TypeFacts::new(definitions, HashMap::new(), Default::default(), [&body]).unwrap();

    assert!(facts.is_drop_instantiation("Duo", &[string(), parameter("B")]));
}

mod drop_hook_reach {
    //! Whether releasing a value may run a drop hook anywhere inside it.

    use super::{generic_class, instance, parameter, string, ty};
    use miri::ast::factory::type_expr_non_null;
    use miri::ast::types::{Type, TypeKind};
    use miri::ast::MemberVisibility;
    use miri::mir::type_facts::{
        DropHookReach, EnumDefinition, MethodInfo, StructDefinition, TypeDefinition,
    };
    use miri::type_checker::context::TraitDefinition;
    use std::collections::{BTreeMap, HashMap};

    fn named(name: &str) -> Type {
        ty(TypeKind::Custom(name.to_string(), None))
    }

    fn drop_hook() -> MethodInfo {
        MethodInfo {
            params: Vec::new(),
            is_out_flags: Vec::new(),
            return_type: ty(TypeKind::Void),
            visibility: MemberVisibility::Public,
            is_constructor: false,
            is_abstract: false,
            is_static: false,
            attributes: Vec::new(),
        }
    }

    /// A plain class `name` holding one `field`, extending `base`, with a
    /// drop hook when `has_hook`.
    fn class(name: &str, field: Type, base: Option<&str>, has_hook: bool) -> TypeDefinition {
        let TypeDefinition::Class(mut def) = generic_class(name, field) else {
            panic!("`generic_class` builds a class");
        };
        def.generics = None;
        def.base_class = base.map(str::to_string);
        if has_hook {
            def.methods.insert("drop".to_string(), drop_hook());
        }
        TypeDefinition::Class(def)
    }

    /// `Res` runs a hook; `Box<T>` holds a `T`; `Child` runs a hook and
    /// extends `Base`, which runs none; `Node` holds itself; `Pair` stores a
    /// `Res` field and `Wrap` a `Res` payload; `Shape` is a trait.
    fn definitions() -> HashMap<String, TypeDefinition> {
        let int = ty(TypeKind::Int);
        let optional_node = ty(TypeKind::Option(Box::new(named("Node"))));
        let trait_def = TraitDefinition {
            name: "Shape".to_string(),
            generics: None,
            parent_traits: Vec::new(),
            parent_trait_args: BTreeMap::new(),
            methods: BTreeMap::new(),
            module: String::new(),
        };
        let pair = StructDefinition {
            fields: vec![("res".to_string(), named("Res"), MemberVisibility::Public)],
            generics: None,
            module: String::new(),
        };
        let wrap = EnumDefinition {
            variants: BTreeMap::from([("Held".to_string(), vec![named("Res")])]),
            generics: None,
            methods: BTreeMap::new(),
            module: String::new(),
            must_use: false,
            non_exhaustive: false,
        };
        HashMap::from([
            ("Res".to_string(), class("Res", int.clone(), None, true)),
            ("Box".to_string(), generic_class("Box", parameter("T"))),
            ("Base".to_string(), class("Base", int.clone(), None, false)),
            ("Child".to_string(), class("Child", int, Some("Base"), true)),
            (
                "Node".to_string(),
                class("Node", optional_node, None, false),
            ),
            ("Pair".to_string(), TypeDefinition::Struct(pair)),
            ("Wrap".to_string(), TypeDefinition::Enum(wrap)),
            ("Shape".to_string(), TypeDefinition::Trait(trait_def)),
        ])
    }

    #[test]
    fn a_value_holding_a_hooked_value_anywhere_may_run_its_hook() {
        let definitions = definitions();
        let reach = DropHookReach::new(&definitions);
        let tuple = ty(TypeKind::Tuple(vec![
            type_expr_non_null(ty(TypeKind::Int)),
            type_expr_non_null(named("Res")),
        ]));
        for held in [
            named("Res"),
            ty(TypeKind::Option(Box::new(named("Res")))),
            tuple,
            instance("Box", vec![named("Res")]),
            named("Pair"),
            named("Wrap"),
        ] {
            assert!(reach.may_run_drop_hook(&held), "{held:?} reaches `Res`");
        }
    }

    #[test]
    fn a_value_whose_dynamic_type_is_unknown_may_run_a_hook() {
        let definitions = definitions();
        let reach = DropHookReach::new(&definitions);
        assert!(
            reach.may_run_drop_hook(&named("Base")),
            "a `Base` may be a `Child`"
        );
        assert!(reach.may_run_drop_hook(&named("Shape")), "a trait value");
        assert!(reach.may_run_drop_hook(&parameter("T")), "a type parameter");
        assert!(
            reach.may_run_drop_hook(&named("Undeclared")),
            "an unknown name"
        );
    }

    #[test]
    fn a_value_holding_nothing_hooked_runs_no_hook() {
        let definitions = definitions();
        let reach = DropHookReach::new(&definitions);
        for held in [
            string(),
            instance("Box", vec![string()]),
            named("Node"),
            ty(TypeKind::Option(Box::new(string()))),
        ] {
            assert!(!reach.may_run_drop_hook(&held), "{held:?} reaches no hook");
        }
    }
}
