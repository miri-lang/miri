// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::type_checker::escape_analysis::*;
use std::collections::{BTreeSet, HashMap};

#[test]
fn default_summary_is_empty() {
    let s = EscapeSummary::default();
    assert!(s.is_empty());
    assert!(s.direct_escapes.is_empty());
    assert!(s.conditional_escapes.is_empty());
    assert!(s.return_aliases.is_empty());
}

#[test]
fn direct_escape_membership() {
    let mut s = EscapeSummary::default();
    s.direct_escapes.insert(0);
    s.direct_escapes.insert(2);
    assert!(s.directly_escapes(0));
    assert!(!s.directly_escapes(1));
    assert!(s.directly_escapes(2));
    assert!(!s.is_empty());
}

#[test]
fn conditional_escape_roundtrip() {
    let ce = ConditionalEscape {
        param: 0,
        via_fn_param: 1,
        callee_param: 0,
    };
    let mut s = EscapeSummary::default();
    s.conditional_escapes.push(ce.clone());
    assert_eq!(s.conditional_escapes.len(), 1);
    assert_eq!(s.conditional_escapes[0], ce);
    assert!(!s.is_empty());
}

#[test]
fn return_aliases_membership() {
    let mut s = EscapeSummary::default();
    s.return_aliases.insert(1);
    assert!(s.return_aliases.contains(&1));
    assert!(!s.return_aliases.contains(&0));
}

#[test]
fn equality_and_clone() {
    let mut a = EscapeSummary::default();
    a.direct_escapes.insert(0);
    let b = a.clone();
    assert_eq!(a, b);
    a.direct_escapes.insert(1);
    assert_ne!(a, b);
}

// ── FFI summary loading tests ────────────────────────────────────────────────

#[test]
fn load_ffi_summaries_parses_without_panic() {
    let summaries = load_ffi_summaries();
    assert!(
        !summaries.is_empty(),
        "escape_summaries.toml should have at least one entry"
    );
}

#[test]
fn list_push_escapes_element() {
    let summaries = load_ffi_summaries();
    let s = summaries
        .get("miri_rt_list_push")
        .expect("miri_rt_list_push must have a summary");
    // param 1 (val) escapes into the list
    assert!(s.directly_escapes(1), "val (param 1) must escape");
    // param 0 (the raw list pointer) is unmanaged — not listed
    assert!(!s.directly_escapes(0));
    assert!(s.conditional_escapes.is_empty());
    assert!(s.return_aliases.is_empty());
}

#[test]
fn map_set_escapes_key_and_value() {
    let summaries = load_ffi_summaries();
    let s = summaries
        .get("miri_rt_map_set")
        .expect("miri_rt_map_set must have a summary");
    assert!(s.directly_escapes(1), "key (param 1) must escape");
    assert!(s.directly_escapes(2), "value (param 2) must escape");
    assert!(!s.directly_escapes(0));
}

#[test]
fn set_add_escapes_element() {
    let summaries = load_ffi_summaries();
    let s = summaries
        .get("miri_rt_set_add")
        .expect("miri_rt_set_add must have a summary");
    assert!(s.directly_escapes(1), "elem (param 1) must escape");
    assert!(!s.directly_escapes(0));
}

#[test]
fn io_sinks_have_no_escapes() {
    let summaries = load_ffi_summaries();
    for name in &[
        "miri_rt_print",
        "miri_rt_println",
        "miri_rt_eprint",
        "miri_rt_eprintln",
    ] {
        let s = summaries
            .get(*name)
            .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
        assert!(
            s.is_empty(),
            "{name} is an IO sink — no parameters should escape"
        );
    }
}

#[test]
fn list_insert_and_set_escape_element() {
    let summaries = load_ffi_summaries();
    for name in &["miri_rt_list_insert", "miri_rt_list_set"] {
        let s = summaries
            .get(*name)
            .unwrap_or_else(|| panic!("{name} must have a summary"));
        assert!(s.directly_escapes(2), "{name}: val (param 2) must escape");
    }
}

#[test]
fn array_set_val_escapes_element() {
    let summaries = load_ffi_summaries();
    let s = summaries
        .get("miri_rt_array_set_val")
        .expect("miri_rt_array_set_val must have a summary");
    assert!(s.directly_escapes(2), "val (param 2) must escape");
    assert!(!s.directly_escapes(0));
    assert!(!s.directly_escapes(1));
}

#[test]
fn map_read_only_accessors_have_no_escapes() {
    let summaries = load_ffi_summaries();
    for name in &[
        "miri_rt_map_get",
        "miri_rt_map_contains_key",
        "miri_rt_map_remove",
    ] {
        let s = summaries
            .get(*name)
            .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
        assert!(
            s.is_empty(),
            "{name} is a read-only accessor — no parameters should escape"
        );
    }
}

#[test]
fn set_read_only_accessors_have_no_escapes() {
    let summaries = load_ffi_summaries();
    for name in &["miri_rt_set_contains", "miri_rt_set_remove"] {
        let s = summaries
            .get(*name)
            .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
        assert!(
            s.is_empty(),
            "{name} is a read-only accessor — no parameters should escape"
        );
    }
}

// ── Value-flow rule: analyze_return_value ───────────────────────────────────
//
// These tests cover each of the 7 enumerated rule cases by
// hand-building small return expressions and the supporting types map.
// They exercise the analyzer in isolation; integration with the
// call-graph fixpoint is deferred.

use miri::ast::expression::{Expression, ExpressionKind};
use miri::ast::factory::{
    call_with_span, conditional_with_span, expr_with_span, identifier_with_span, index_with_span,
    list_with_span, make_type, member_with_span, tuple_with_span,
};
use miri::ast::statement::IfStatementType;
use miri::ast::types::{Type, TypeKind};
use miri::ast::Parameter;
use miri::error::syntax::Span;
use miri::type_checker::context::TypeDefinition;

/// Builds a `Parameter` with the given name and a placeholder type
/// expression — the analyzer never reads `Parameter::typ`, only `name`.
fn param(name: &str) -> Parameter {
    Parameter {
        name: name.to_string(),
        typ: Box::new(expr_with_span(
            ExpressionKind::Type(Box::new(make_type(TypeKind::Int)), false),
            Span::new(0, 0),
        )),
        guard: None,
        default_value: None,
        is_out: false,
    }
}

/// Managed type used as a stand-in for "any heap type": `Custom("List", _)`
/// with no `type_definitions` entry → `is_auto_copy` returns `false`, so
/// the analyzer treats it as managed (alias-creating).
fn managed_type() -> Type {
    make_type(TypeKind::Custom("List".to_string(), None))
}

fn primitive_type() -> Type {
    make_type(TypeKind::Int)
}

/// Creates an `Identifier` expression with the given name and registers a
/// type for it in `types`.
fn ident(name: &str, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
    let e = identifier_with_span(name, Span::new(0, 0));
    types.insert(e.id, ty);
    e
}

/// Wraps `expr` in a fresh outer expression `Index(expr, _)` registering
/// `result_ty` (the element type) at the new node's id.
fn index(
    obj: Expression,
    idx: Expression,
    result_ty: Type,
    types: &mut HashMap<usize, Type>,
) -> Expression {
    let e = index_with_span(obj, idx, Span::new(0, 0));
    types.insert(e.id, result_ty);
    e
}

fn member(
    obj: Expression,
    field_name: &str,
    result_ty: Type,
    types: &mut HashMap<usize, Type>,
) -> Expression {
    let field = identifier_with_span(field_name, Span::new(0, 0));
    let e = member_with_span(obj, field, Span::new(0, 0));
    types.insert(e.id, result_ty);
    e
}

fn list_lit(elems: Vec<Expression>, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
    let e = list_with_span(elems, Span::new(0, 0));
    types.insert(e.id, ty);
    e
}

fn tuple_lit(elems: Vec<Expression>, ty: Type, types: &mut HashMap<usize, Type>) -> Expression {
    let e = tuple_with_span(elems, Span::new(0, 0));
    types.insert(e.id, ty);
    e
}

/// Builds a call `name(args)` where `name` is a free function identifier.
/// The callee identifier and the call expression are typed as managed by
/// default to keep the alias chain alive; a primitive return type can be
/// supplied via `return_ty` to test rule 6.
fn call(
    name: &str,
    args: Vec<Expression>,
    return_ty: Type,
    types: &mut HashMap<usize, Type>,
) -> Expression {
    let callee = identifier_with_span(name, Span::new(0, 0));
    // Callee identifier itself doesn't matter for type-tracking; it is not
    // an Identifier *of a parameter* in the tests below, so the analyzer
    // ignores its type.  Register a placeholder for completeness.
    types.insert(callee.id, primitive_type());
    let e = call_with_span(callee, args, Span::new(0, 0));
    types.insert(e.id, return_ty);
    e
}

fn empty_summaries() -> HashMap<FunctionId, EscapeSummary> {
    HashMap::new()
}

// ── Rule 1: `return p` ────────────────────────────────────────────────────

#[test]
fn rule1_return_managed_param_escapes_and_aliases() {
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("items")];
    let ret = ident("items", managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.contains(&0));
    assert!(flow.return_aliases.contains(&0));
}

#[test]
fn rule1_return_primitive_param_does_not_escape() {
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("n")];
    let ret = ident("n", primitive_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Rule 2: aggregate construction ────────────────────────────────────────

#[test]
fn rule2_return_list_of_managed_params_escapes_each() {
    // `return [p, q]` where p, q are managed parameters.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p"), param("q")];
    let p = ident("p", managed_type(), &mut types);
    let q = ident("q", managed_type(), &mut types);
    let ret = list_lit(vec![p, q], managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.contains(&0));
    assert!(flow.direct_escapes.contains(&1));
    assert!(flow.return_aliases.contains(&0));
    assert!(flow.return_aliases.contains(&1));
}

#[test]
fn rule2_return_tuple_of_managed_params_escapes_each() {
    // `return Pair(p, q)` represented as a tuple constructor.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p"), param("q")];
    let p = ident("p", managed_type(), &mut types);
    let q = ident("q", managed_type(), &mut types);
    let ret = tuple_lit(vec![p, q], managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert_eq!(
        flow.direct_escapes,
        BTreeSet::from([0_usize, 1_usize]),
        "both params must be in direct_escapes"
    );
    assert_eq!(flow.return_aliases, BTreeSet::from([0_usize, 1_usize]));
}

// ── Rule 3: `return p[i]` — managed vs primitive element ──────────────────

#[test]
fn rule3_index_managed_element_escapes() {
    // `return p[i]` where p has type List<List<int>>; element type managed.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p"), param("i")];
    let p = ident("p", managed_type(), &mut types);
    let i = ident("i", primitive_type(), &mut types);
    let ret = index(p, i, managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.contains(&0));
    assert!(flow.return_aliases.contains(&0));
    // The integer index parameter does not flow into the return.
    assert!(!flow.direct_escapes.contains(&1));
}

#[test]
fn rule3_index_primitive_element_does_not_escape() {
    // `return p[i]` where p has type List<int>; element type is auto-copy.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p"), param("i")];
    let p = ident("p", managed_type(), &mut types);
    let i = ident("i", primitive_type(), &mut types);
    let ret = index(p, i, primitive_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Rule 4: `return p.field` — managed vs primitive field ────────────────

#[test]
fn rule4_member_managed_field_escapes() {
    // `return p.cache` where cache: List<int>.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let ret = member(p, "cache", managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.contains(&0));
    assert!(flow.return_aliases.contains(&0));
}

#[test]
fn rule4_member_primitive_field_does_not_escape() {
    // `return p.count` where count: int.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let ret = member(p, "count", primitive_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Rule 5: `return f(p)` where f consumes param 0 ──────────────────────

#[test]
fn rule5_call_consumes_param_via_sink_chain() {
    // `return store(p)` where `store`'s param 0 is in direct_escapes.
    // Expected: p in direct_escapes (consumed via f's sink), but the
    // call's return value is independent of p's heap (f.return_aliases
    // is empty), so p is NOT in our return_aliases.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    summaries.insert(
        "store".to_string(),
        EscapeSummary {
            direct_escapes: BTreeSet::from([0_usize]),
            ..EscapeSummary::default()
        },
    );
    let ret = call("store", vec![p], managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(
        flow.direct_escapes.contains(&0),
        "p must be in direct_escapes (consumed by f's sink chain)"
    );
    assert!(
        !flow.return_aliases.contains(&0),
        "p must NOT be in return_aliases (f's return is independent of p's heap)"
    );
}

// ── Rule 6: `return f(p)` where f neither escapes nor return-aliases 0 ─

#[test]
fn rule6_call_neither_consumes_nor_aliases() {
    // `return length_of(p)` where length_of has empty escape summary.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    summaries.insert("length_of".to_string(), EscapeSummary::default());
    let ret = call("length_of", vec![p], primitive_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Rule 7: `return f(p)` where f.return_aliases ∋ 0 ────────────────────

#[test]
fn rule7_call_return_aliases_param_propagates_alias() {
    // `return identity(p)` where `identity`'s return aliases param 0.
    // Expected: p in BOTH direct_escapes and return_aliases (the call's
    // return value aliases p's heap, and our return is that call's value).
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    summaries.insert(
        "identity".to_string(),
        EscapeSummary {
            return_aliases: BTreeSet::from([0_usize]),
            ..EscapeSummary::default()
        },
    );
    let ret = call("identity", vec![p], managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(flow.direct_escapes.contains(&0));
    assert!(flow.return_aliases.contains(&0));
}

#[test]
fn rule7_call_return_aliases_only_when_outer_return_alias_holds() {
    // Even if a callee's return aliases its arg, that does not propagate
    // to *our* return when the call's value does not flow into our return
    // (e.g., the call appears under an auto-copy projection).
    // `return identity(p).length` — `.length` is primitive, so the call's
    // managed return is dropped at the projection step.  No aliasing
    // contribution from the call.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    summaries.insert(
        "identity".to_string(),
        EscapeSummary {
            return_aliases: BTreeSet::from([0_usize]),
            ..EscapeSummary::default()
        },
    );
    let inner_call = call("identity", vec![p], managed_type(), &mut types);
    let ret = member(inner_call, "length", primitive_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(
        flow.direct_escapes.is_empty(),
        "primitive projection breaks the alias chain — p must not escape"
    );
    assert!(flow.return_aliases.is_empty());
}

// ── Bonus coverage: behaviour at unresolved callees ────────────────────────
//
// The conservative policy ("every managed param escapes") for unresolved
// callees is the escape analysis pass's responsibility, not this value-flow
// analyzer's.  In isolation, the analyzer makes no escape claim for an
// unresolved callee — it simply does not propagate the alias context.
// This guard pins that behaviour so the pass has a known baseline.

#[test]
fn unresolved_callee_makes_no_escape_claim() {
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let p = ident("p", managed_type(), &mut types);
    // Empty summaries map — the analyzer does NOT find `unknown_fn`.
    let summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    let ret = call("unknown_fn", vec![p], managed_type(), &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(
        flow.direct_escapes.is_empty(),
        "the value-flow analyzer alone does not enforce the conservative default for unresolved callees"
    );
    assert!(flow.return_aliases.is_empty());
}

// ── Identifier referring to a non-parameter is ignored ────────────────────

#[test]
fn return_local_variable_is_ignored() {
    // The analyzer only classifies parameter identifiers; a return of a
    // local variable contributes nothing to the parameter-indexed flow.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let local = ident("local", managed_type(), &mut types);

    let flow = analyze_return_value(&local, &params, &types, &HashMap::new(), &empty_summaries());

    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Auto-copy struct test: managed-looking by name but auto-copy ─────────
//
// When a parameter's type is a small POD struct registered in
// `type_definitions` whose fields are all primitives, `is_auto_copy`
// returns true and the alias chain breaks at the param identifier itself.
// This pins down the "primitive types do not escape" half of rule 1.

#[test]
fn auto_copy_struct_param_does_not_escape() {
    use miri::type_checker::context::StructDefinition;

    let mut type_defs: HashMap<String, TypeDefinition> = HashMap::new();
    type_defs.insert(
        "Point".to_string(),
        TypeDefinition::Struct(StructDefinition {
            fields: vec![
                (
                    "x".to_string(),
                    make_type(TypeKind::Int),
                    miri::ast::common::MemberVisibility::Public,
                ),
                (
                    "y".to_string(),
                    make_type(TypeKind::Int),
                    miri::ast::common::MemberVisibility::Public,
                ),
            ],
            generics: None,
            module: "test".to_string(),
            has_drop: false,
        }),
    );
    let point_ty = make_type(TypeKind::Custom("Point".to_string(), None));
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p")];
    let ret = ident("p", point_ty, &mut types);

    let flow = analyze_return_value(&ret, &params, &types, &type_defs, &empty_summaries());

    assert!(
        flow.direct_escapes.is_empty(),
        "auto-copy struct param does not flow into return alias"
    );
    assert!(flow.return_aliases.is_empty());
}

// ── Conditional in return position ────────────────────────────────────────
//
// Regression guard for the field-order bug in `ExpressionKind::Conditional`:
// the variant carries `(then, cond, else?)`, not `(cond, then, else?)`.
// A both-branches-managed-param test catches a swap because it requires
// both arms to be walked with `aliases_return=true`; a swap would silently
// walk the then-branch with `false` and miss the escape.

#[test]
fn conditional_branches_propagate_alias_to_both() {
    // `return (cond ? p : q)` where p, q are managed params; cond is some
    // primitive expression that does not reference p or q.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("p"), param("q"), param("cond_local")];
    let p = ident("p", managed_type(), &mut types);
    let q = ident("q", managed_type(), &mut types);
    let cond = ident("cond_local", primitive_type(), &mut types);
    let if_expr = conditional_with_span(p, cond, Some(q), IfStatementType::If, Span::new(0, 0));
    types.insert(if_expr.id, managed_type());

    let flow = analyze_return_value(
        &if_expr,
        &params,
        &types,
        &HashMap::new(),
        &empty_summaries(),
    );

    assert!(
        flow.direct_escapes.contains(&0),
        "then-branch param p must be in direct_escapes"
    );
    assert!(
        flow.direct_escapes.contains(&1),
        "else-branch param q must be in direct_escapes"
    );
    assert!(
        !flow.direct_escapes.contains(&2),
        "the condition expression's identifier must not flow into the value"
    );
    assert!(flow.return_aliases.contains(&0));
    assert!(flow.return_aliases.contains(&1));
}

#[test]
fn conditional_managed_param_in_condition_does_not_escape() {
    // Catches the inverse of the field-order bug: a managed-typed param
    // referenced in the *condition* must NOT escape via the return value.
    // The current canonical layout is `Conditional(then, cond, else?)`.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("flag"), param("a"), param("b")];
    // `flag` is managed (e.g. an Option<bool>) — alias-creating IF wrongly
    // walked with aliases_return=true.
    let flag = ident("flag", managed_type(), &mut types);
    let a = ident("a", primitive_type(), &mut types);
    let b = ident("b", primitive_type(), &mut types);
    let if_expr = conditional_with_span(a, flag, Some(b), IfStatementType::If, Span::new(0, 0));
    // Result type is primitive → the conditional's value cannot alias
    // anyone's heap, but the analyzer should reach this conclusion
    // regardless of result type because the *branches* are primitive.
    types.insert(if_expr.id, primitive_type());

    let flow = analyze_return_value(
        &if_expr,
        &params,
        &types,
        &HashMap::new(),
        &empty_summaries(),
    );

    assert!(
        !flow.direct_escapes.contains(&0),
        "param `flag` is only in the condition — it must not escape"
    );
    assert!(flow.direct_escapes.is_empty());
    assert!(flow.return_aliases.is_empty());
}

// ── Method dispatch summary lookup (`ClassName_method` key) ───────────────
//
// Pins the resolve_callee_summary path for `obj.method(p)`: the receiver
// becomes summary slot 0, and `args` shift by 1.

#[test]
fn method_call_consumes_receiver_via_class_method_key() {
    // `return cache.store(p)` where Cache_store has direct_escapes = {0, 1}.
    // Receiver `cache` is summary slot 0, arg `p` is slot 1; both must be
    // marked direct-escape via rule 5.
    let mut types: HashMap<usize, Type> = HashMap::new();
    let params = vec![param("cache"), param("p")];
    let cache = ident(
        "cache",
        make_type(TypeKind::Custom("Cache".to_string(), None)),
        &mut types,
    );
    let p = ident("p", managed_type(), &mut types);
    let store_method = identifier_with_span("store", Span::new(0, 0));
    types.insert(store_method.id, primitive_type());
    let callee = member_with_span(cache, store_method, Span::new(0, 0));
    types.insert(callee.id, primitive_type());
    let ret = call_with_span(callee, vec![p], Span::new(0, 0));
    types.insert(ret.id, managed_type());

    let mut summaries: HashMap<FunctionId, EscapeSummary> = HashMap::new();
    summaries.insert(
        "Cache_store".to_string(),
        EscapeSummary {
            direct_escapes: BTreeSet::from([0_usize, 1_usize]),
            ..EscapeSummary::default()
        },
    );

    let flow = analyze_return_value(&ret, &params, &types, &HashMap::new(), &summaries);

    assert!(
        flow.direct_escapes.contains(&0),
        "receiver `cache` must be marked direct-escape via Cache_store slot 0"
    );
    assert!(
        flow.direct_escapes.contains(&1),
        "arg `p` must be marked direct-escape via Cache_store slot 1"
    );
    assert!(flow.return_aliases.is_empty());
}
