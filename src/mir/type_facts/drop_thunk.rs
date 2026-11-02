// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which drop thunk releases a value of a named type: the one bare thunk of a
//! type without parameters, the shared thunk of a generic type reached where
//! its arguments are still open, or the thunk of the concrete instantiation it
//! was built at.

use super::TypeFacts;
use crate::ast::expression::Expression;
use crate::ast::types::Type;
use crate::mir::instantiation::{self, shape};
use crate::mir::symbol::{Symbol, ThunkKind};
use crate::type_checker::context::TypeDefinition;
use std::collections::{HashMap, HashSet};

/// Why no drop thunk may release a value: releasing it through any thunk
/// would leak what its fields hold, or name a thunk nothing defines.
#[derive(Debug, Clone, PartialEq)]
pub struct DropThunkRefusal(pub String);

/// The drop thunk a value of a named type is released through.
#[derive(Debug, Clone, PartialEq)]
pub enum DropThunk {
    /// A type that declares no parameters: its one bare thunk.
    Shared,
    /// A generic type reached where its arguments are still open — no
    /// arguments at all, or ones naming a parameter of the body it is held in
    /// (`Box<T>` inside a shared generic body). The bare thunk skips every
    /// field written at a parameter.
    SharedBody,
    /// A concrete instantiation, released by the thunk emitted for these
    /// arguments.
    PerInstantiation(Vec<Type>),
}

impl DropThunk {
    /// The arguments of the per-instantiation thunk, or `None` for the bare one.
    pub fn per_instantiation(self) -> Option<Vec<Type>> {
        match self {
            DropThunk::PerInstantiation(args) => Some(args),
            DropThunk::Shared | DropThunk::SharedBody => None,
        }
    }
}

/// Which drop thunk releases a value of `name` carrying `type_args`.
///
/// A generic struct, class and enum declare fields written in their own
/// parameters, so the field an instantiation stores is known only once the
/// arguments are substituted, and each instantiation a value is held at
/// gets its own thunk ([`TypeFacts::drop_instantiations_of`]). That holds
/// too for an instantiation inference left partly unbound
/// (`Result<String, E>` from `let r = Result.Ok("s")`): its thunk releases
/// what the bound arguments hold and skips the unbound slot, which a value
/// built that way never fills.
///
/// Only code that leaves parameters open — a shared generic body, a closure
/// inside one, or a generic type's own shared thunk — releases a value at
/// an instantiation naming one of those parameters, through the shared
/// thunk (the open parameters it is handed). Anything else is reported rather than
/// released by a thunk that skips every field written at a parameter and
/// leaks what it holds: a parameter the body's substitution bound
/// (the bound parameters it is handed), a parameter neither left open nor unbound,
/// an instantiation no thunk is emitted for, and an argument that is
/// neither a type nor a value.
///
/// [`TypeFacts::drop_instantiations_of`]: crate::mir::type_facts::TypeFacts::drop_instantiations_of
pub(crate) fn drop_thunk(
    facts: &TypeFacts,
    name: &str,
    type_args: Option<&[Expression]>,
    open_params: &HashSet<String>,
    bound_params: &HashSet<String>,
) -> Result<DropThunk, DropThunkRefusal> {
    let is_generic = facts
        .definitions()
        .get(name)
        .and_then(TypeDefinition::generics)
        .is_some();
    if !is_generic {
        return Ok(DropThunk::Shared);
    }
    // A generic value typed with no arguments at all is released through
    // the shared body, which skips every field written at a parameter. In
    // a body that leaves parameters open that is the body's own value. In
    // a concrete body the only such value is a payload-less enum variant
    // written where no location names an instantiation
    // (`let n = Holder.Nothing`): the checker refuses to store a payload
    // into a binding typed that way, so nothing is skipped. Anything else
    // typed bare there was built at no instantiation, and releasing it
    // would leak what its parameter-typed fields hold.
    let Some(arg_exprs) = type_args.filter(|args| !args.is_empty()) else {
        let is_enum = matches!(facts.definitions().get(name), Some(TypeDefinition::Enum(_)));
        if open_params.is_empty() && !is_enum {
            return Err(DropThunkRefusal(format!(
                "a `{name}` value is released in a concrete body with no type arguments, \
                 so the fields written at its parameters would never be released"
            )));
        }
        return Ok(DropThunk::SharedBody);
    };
    let (args, open) = instantiation_arguments(name, arg_exprs, facts)?;
    let args = shape::with_unbound_arguments_normalized(facts.definitions(), name, &args);
    if let Some(missed) = open.iter().find(|param| bound_params.contains(*param)) {
        return Err(DropThunkRefusal(format!(
            "a `{name}` value is released at an instantiation naming the parameter \
             `{missed}`, which the substitution of the body releasing it bound"
        )));
    }
    let strays: Vec<&String> = open
        .iter()
        .filter(|param| !open_params.contains(*param))
        .collect();
    if strays.is_empty() && !open.is_empty() {
        // TODO: the shared method body of a value-generic class
        // (`Buf<T, Size>`) drops its own type here, through the bare thunk,
        // which skips the fields written at `T`; that is the likely root of
        // the leak a value-generic class holding `Array<T, Size>` beside a
        // bare `T` shows at a managed instantiation.
        return Ok(DropThunk::SharedBody);
    }
    let definitions = facts.definitions();
    if let Some(stray) = strays
        .iter()
        .find(|param| !is_unbound_parameter(name, &args, param, definitions))
    {
        return Err(DropThunkRefusal(format!(
            "a `{name}` value is released at an instantiation naming the parameter \
             `{stray}`, which the code releasing it does not leave open"
        )));
    }
    let is_named = args.len() == arg_exprs.len()
        && shape::is_partially_bound_instantiation(definitions, name, &args);
    if strays.len() < open.len() || !is_named {
        // Every argument is open here or unbound: nothing is stored at a
        // bound argument for a per-instantiation thunk to release.
        return Ok(DropThunk::SharedBody);
    }
    if facts.is_drop_instantiation(name, &args) {
        return Ok(DropThunk::PerInstantiation(args));
    }
    Err(DropThunkRefusal(format!(
        "a `{name}` value is released at {}, an instantiation no drop thunk is emitted for",
        Symbol::type_thunk(ThunkKind::Drop, name, &args).written()
    )))
}

/// Whether `param` appears among `args` only as `name`'s own parameter at
/// its declared position — an argument inference left unbound.
fn is_unbound_parameter(
    name: &str,
    args: &[Type],
    param: &str,
    definitions: &HashMap<String, TypeDefinition>,
) -> bool {
    let mut is_unbound = false;
    for (position, arg) in args.iter().enumerate() {
        if shape::is_own_parameter_at(definitions, name, position, arg) {
            is_unbound |= shape::open_parameter_names(arg, definitions)
                .iter()
                .any(|spelled| spelled == param);
        } else if shape::open_parameter_names(arg, definitions)
            .iter()
            .any(|spelled| spelled == param)
        {
            return false;
        }
    }
    is_unbound
}

/// `arg_exprs` as the instantiation arguments a drop thunk is named by,
/// with the name of every parameter they leave open — a type parameter
/// anywhere inside a type argument, or one an unfolded value argument
/// (`Size`, `Size + 1`) reads.
fn instantiation_arguments(
    name: &str,
    arg_exprs: &[Expression],
    facts: &TypeFacts,
) -> Result<(Vec<Type>, Vec<String>), DropThunkRefusal> {
    let mut args = Vec::with_capacity(arg_exprs.len());
    let mut open = Vec::new();
    for arg in arg_exprs {
        match instantiation::instantiation_argument(arg) {
            Some(ty) => {
                open.extend(facts.open_parameter_names(&ty));
                args.push(ty);
            }
            None => {
                let names = shape::value_parameter_names(arg).ok_or_else(|| {
                    DropThunkRefusal(format!(
                        "a `{name}` value carries a type argument that is neither a type \
                         nor a value: {:?}",
                        arg.node
                    ))
                })?;
                open.extend(names);
            }
        }
    }
    Ok((args, open))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::expression::ExpressionKind;
    use crate::ast::factory::type_expr_non_null;
    use crate::ast::literal::Literal;
    use crate::ast::types::TypeDeclarationKind;
    use crate::ast::types::TypeKind;
    use crate::ast::{IdNode, MemberVisibility};
    use crate::error::syntax::Span;
    use crate::mir::type_facts::{ClassDefinition, FieldInfo, GenericDefinition};
    use std::collections::{BTreeMap, BTreeSet};

    fn ty(kind: TypeKind) -> Type {
        Type::new(kind, Span::default())
    }

    fn class(name: &str, generic: bool) -> TypeDefinition {
        let parameter = ty(TypeKind::Generic(
            "T".to_string(),
            None,
            TypeDeclarationKind::None,
        ));
        TypeDefinition::Class(ClassDefinition {
            name: name.to_string(),
            generics: generic.then(|| {
                vec![GenericDefinition {
                    name: "T".to_string(),
                    constraint: None,
                    kind: TypeDeclarationKind::None,
                }]
            }),
            base_class: None,
            base_class_args: None,
            traits: Vec::new(),
            trait_args: HashMap::new(),
            fields: vec![(
                "value".to_string(),
                FieldInfo {
                    ty: if generic {
                        parameter
                    } else {
                        ty(TypeKind::Int)
                    },
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

    /// `class Duo<A, B>`: two parameters, so one can be bound while inference
    /// leaves the other unbound.
    fn duo() -> TypeDefinition {
        let TypeDefinition::Class(mut def) = class("Duo", true) else {
            panic!("`class` builds a class");
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

    /// Facts over a generic `Box<T>`, a plain `Plain` and a two-parameter
    /// `Duo<A, B>`, where `Box<String>` and `Duo<String, B>` are the
    /// instantiations a value is released at.
    fn facts() -> TypeFacts {
        let definitions = HashMap::from([
            ("Box".to_string(), class("Box", true)),
            ("Plain".to_string(), class("Plain", false)),
            ("Duo".to_string(), duo()),
        ]);
        let registry = HashMap::from([
            ("Box".to_string(), vec![vec![ty(TypeKind::String)]]),
            (
                "Duo".to_string(),
                vec![vec![ty(TypeKind::String), generic("B")]],
            ),
        ]);
        let Ok(facts) = TypeFacts::new(definitions, registry, Default::default(), []) else {
            panic!("a `Box<String>` registry settles");
        };
        facts
    }

    /// The thunk chosen for `name` at `args` in code leaving `open` open.
    fn choose_in(
        facts: &TypeFacts,
        name: &str,
        args: Option<&[Expression]>,
        open: &[&str],
    ) -> Result<DropThunk, DropThunkRefusal> {
        let open_params: HashSet<String> = open.iter().map(|name| name.to_string()).collect();
        drop_thunk(facts, name, args, &open_params, &HashSet::new())
    }

    /// The thunk chosen for `name` at `args` in code leaving nothing open.
    fn choose(
        facts: &TypeFacts,
        name: &str,
        args: Option<&[Expression]>,
    ) -> Result<DropThunk, DropThunkRefusal> {
        choose_in(facts, name, args, &[])
    }

    fn generic(name: &str) -> Type {
        ty(TypeKind::Generic(
            name.to_string(),
            None,
            TypeDeclarationKind::None,
        ))
    }

    /// The thunk chosen for `name` at `args` in a body whose substitution
    /// bound `bound`.
    fn choose_bound(
        facts: &TypeFacts,
        name: &str,
        args: Option<&[Expression]>,
        bound: &[&str],
    ) -> Result<DropThunk, DropThunkRefusal> {
        let bound_params: HashSet<String> = bound.iter().map(|name| name.to_string()).collect();
        drop_thunk(facts, name, args, &HashSet::new(), &bound_params)
    }

    #[test]
    fn a_parameter_the_body_substitution_bound_is_refused_as_a_missed_substitution() {
        let args = [open_parameter()];
        let Err(DropThunkRefusal(message)) = choose_bound(&facts(), "Box", Some(&args), &["T"])
        else {
            panic!("a leftover bound parameter must be reported");
        };
        assert!(message.contains("substitution of the body"), "{message}");
    }

    #[test]
    fn a_partly_unbound_instantiation_is_released_by_its_own_thunk() {
        let args = [type_arg(TypeKind::String), type_expr_non_null(generic("B"))];
        assert_eq!(
            choose(&facts(), "Duo", Some(&args)).ok(),
            Some(DropThunk::PerInstantiation(vec![
                ty(TypeKind::String),
                generic("B")
            ]))
        );
    }

    #[test]
    fn a_parameter_at_another_types_position_is_not_taken_for_unbound() {
        // `B` is `Duo`'s second parameter, not its first: nothing but an open
        // body could name it there.
        let args = [type_expr_non_null(generic("B")), type_arg(TypeKind::String)];
        assert!(choose(&facts(), "Duo", Some(&args)).is_err());
    }

    fn type_arg(kind: TypeKind) -> Expression {
        type_expr_non_null(ty(kind))
    }

    fn expression(node: ExpressionKind) -> Expression {
        IdNode::new(0, node, Span::default())
    }

    #[test]
    fn a_type_without_parameters_is_released_by_its_one_thunk() {
        assert_eq!(
            choose(&facts(), "Plain", None).ok(),
            Some(DropThunk::Shared)
        );
    }

    #[test]
    fn a_generic_type_without_arguments_is_released_by_the_shared_body_thunk_where_left_open() {
        assert_eq!(
            choose_in(&facts(), "Box", None, &["T"]).ok(),
            Some(DropThunk::SharedBody)
        );
    }

    #[test]
    fn a_generic_class_without_arguments_in_a_concrete_body_is_refused() {
        let Err(DropThunkRefusal(message)) = choose(&facts(), "Box", None) else {
            panic!("a bare generic class in a concrete body must be reported");
        };
        assert!(message.contains("no type arguments"), "{message}");
    }

    #[test]
    fn a_recorded_instantiation_is_released_by_its_own_thunk() {
        let args = [type_arg(TypeKind::String)];
        assert_eq!(
            choose(&facts(), "Box", Some(&args)).ok(),
            Some(DropThunk::PerInstantiation(vec![ty(TypeKind::String)]))
        );
    }

    fn open_parameter() -> Expression {
        type_arg(TypeKind::Generic(
            "T".to_string(),
            None,
            TypeDeclarationKind::None,
        ))
    }

    #[test]
    fn an_instantiation_at_a_parameter_the_body_leaves_open_is_released_by_the_shared_body_thunk() {
        let args = [open_parameter()];
        assert_eq!(
            choose_in(&facts(), "Box", Some(&args), &["T"]).ok(),
            Some(DropThunk::SharedBody)
        );
    }

    #[test]
    fn an_instantiation_left_at_the_types_own_parameter_is_released_by_the_shared_body_thunk() {
        // Inference can leave a parameter of the released type unbound in a
        // concrete body (`Result.Ok(0.5)` typed `Result<f32, E>`).
        let args = [open_parameter()];
        assert_eq!(
            choose(&facts(), "Box", Some(&args)).ok(),
            Some(DropThunk::SharedBody)
        );
    }

    #[test]
    fn an_instantiation_at_a_parameter_neither_the_body_nor_the_type_declares_is_refused() {
        let args = [type_arg(TypeKind::Generic(
            "U".to_string(),
            None,
            TypeDeclarationKind::None,
        ))];
        let Err(DropThunkRefusal(message)) = choose(&facts(), "Box", Some(&args)) else {
            panic!("a parameter no enclosing code declares must be reported");
        };
        assert!(message.contains("does not leave open"), "{message}");
    }

    #[test]
    fn an_unfolded_size_argument_the_body_leaves_open_is_released_by_the_shared_body_thunk() {
        let args = [expression(ExpressionKind::Identifier(
            "Size".to_string(),
            None,
        ))];
        assert_eq!(
            choose_in(&facts(), "Box", Some(&args), &["Size"]).ok(),
            Some(DropThunk::SharedBody)
        );
    }

    #[test]
    fn an_unfolded_size_argument_the_body_does_not_leave_open_is_refused() {
        let args = [expression(ExpressionKind::Identifier(
            "Size".to_string(),
            None,
        ))];
        assert!(choose(&facts(), "Box", Some(&args)).is_err());
    }

    #[test]
    fn a_concrete_instantiation_no_thunk_is_emitted_for_is_refused() {
        let args = [type_arg(TypeKind::Float)];
        let Err(DropThunkRefusal(message)) = choose(&facts(), "Box", Some(&args)) else {
            panic!("an unrecorded concrete instantiation must be reported");
        };
        assert!(
            message.contains("no drop thunk is emitted for"),
            "{message}"
        );
    }

    #[test]
    fn an_argument_that_is_neither_a_type_nor_a_value_is_refused() {
        let args = [expression(ExpressionKind::Literal(Literal::Boolean(true)))];
        let Err(DropThunkRefusal(message)) = choose(&facts(), "Box", Some(&args)) else {
            panic!("an argument naming no instantiation must be reported");
        };
        assert!(message.contains("neither a type nor a value"), "{message}");
    }
}
