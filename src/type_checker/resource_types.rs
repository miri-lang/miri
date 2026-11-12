// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which types are resources: a value is a resource when releasing it may
//! run a drop hook.
//!
//! A class or an enum declaring the hook is one. A struct or a class holding
//! a resource in a field is one, the field read at the type its owner's
//! instantiation gives it: `Wrap<Handle>` holds a resource where `Wrap<int>`
//! holds none, so a generic definition is decided per instantiation. A class
//! is also one when the base it extends is one at the arguments its `extends`
//! clause binds.
//!
//! A value held at a trait, or at a class another class extends, may be any
//! class below it, so that type is a resource when some class below it is one:
//! a plain one, or a generic one at an instantiation the program builds.
//!
//! Every answer is decided once over a settled type table and the program's
//! instantiations, as the least fixed point of these rules, so asking about a
//! value is a lookup: a generic definition records which of its parameters
//! make an instance a resource, and an instance is one when an argument at
//! such a position is.

use super::context::{class_implements_trait, ClassDefinition, GenericDefinition, TypeDefinition};
use super::generics::generic_parameter_name;
use super::utils::{has_drop_hook, trait_declares_drop_hook};
use crate::ast::type_arguments::{TypeArgKind, TypeArgument};
use crate::ast::types::{Type, TypeKind};
use std::collections::{BTreeSet, HashMap};

/// The program's generic class instantiations: the argument tuples each
/// generic class is built at, by class name.
pub type Instantiations = HashMap<String, Vec<Vec<Type>>>;

/// Add `class` built at `arguments` to `instantiations`, unless a tuple of
/// the same kinds is listed already (spans aside).
pub fn add_instantiation(instantiations: &mut Instantiations, class: &str, arguments: Vec<Type>) {
    let tuples = instantiations.entry(class.to_string()).or_default();
    let is_listed = tuples
        .iter()
        .any(|listed| (listed.iter().map(|ty| &ty.kind)).eq(arguments.iter().map(|ty| &ty.kind)));
    if !is_listed {
        tuples.push(arguments);
    }
}

/// What the answers are decided over: the type table and the program's
/// generic class instantiations.
#[derive(Clone, Copy)]
struct Program<'p> {
    definitions: &'p HashMap<String, TypeDefinition>,
    instantiations: &'p Instantiations,
}

/// When an instance of one named type is a resource.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Condition {
    /// Every instance is one, whatever its arguments.
    always: bool,
    /// The positions of the type's parameters at which an argument that is a
    /// resource makes the instance one.
    through: BTreeSet<usize>,
}

impl Condition {
    fn always() -> Self {
        Self {
            always: true,
            through: BTreeSet::new(),
        }
    }

    fn through(position: usize) -> Self {
        Self {
            always: false,
            through: BTreeSet::from([position]),
        }
    }

    fn is_never(&self) -> bool {
        !self.always && self.through.is_empty()
    }

    /// An instance is a resource when either condition makes it one.
    fn or(mut self, other: Self) -> Self {
        if self.always || other.always {
            return Self::always();
        }
        self.through.extend(other.through);
        self
    }
}

/// The types that are resources, decided once over a settled type table, so
/// asking about a value is a lookup: a trait's answer otherwise walks every
/// class implementing it.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ResourceTypes {
    /// When a value held at each type is a resource, by type name; a type
    /// absent is never one.
    conditions: HashMap<String, Condition>,
    /// When an instance of exactly each type is a resource, before what a
    /// class extending it holds: what a subclass inherits from its base.
    instances: HashMap<String, Condition>,
}

impl ResourceTypes {
    /// Every resource among the types `type_definitions` declares, a generic
    /// one at each of its instantiations, and a trait or an extended class
    /// over the classes below it at the instantiations `instantiations` lists.
    pub fn of(
        type_definitions: &HashMap<String, TypeDefinition>,
        instantiations: &Instantiations,
    ) -> Self {
        let program = Program {
            definitions: type_definitions,
            instantiations,
        };
        let mut resources = Self::default();
        loop {
            let decided = resources.decide_next(program);
            if decided == resources {
                return resources;
            }
            resources = decided;
        }
    }

    /// Every condition, read off the conditions decided so far. Each only
    /// grows from one round to the next, so the rounds reach a fixed point.
    fn decide_next(&self, program: Program<'_>) -> Self {
        let mut decided = Self::default();
        for (name, definition) in program.definitions {
            let instance = self.instance_condition(name, definition, program.definitions);
            let held = self.held_condition(name, definition, instance.clone(), program);
            if !instance.is_never() {
                decided.instances.insert(name.clone(), instance);
            }
            if !held.is_never() {
                decided.conditions.insert(name.clone(), held);
            }
        }
        decided
    }

    /// Whether a value of `kind` is a resource. A named type is one at the
    /// arguments it is written with; a generic parameter is one when a bound
    /// it declares is, and an unbounded one is not.
    pub fn holds(&self, kind: &TypeKind) -> bool {
        if let TypeKind::Custom(name, args) = kind {
            let args = args.as_deref().unwrap_or_default();
            return self.holds_named(name, |at| args.get(at).and_then(argument_kind));
        }
        if let TypeKind::Generic(_, bounds) = kind {
            return bounds.entail(|bound| self.holds(&bound.ty.kind));
        }
        false
    }

    /// Whether an instance of the type `name` is a resource, its argument at
    /// each position read through `argument_at`.
    fn holds_named<'k>(
        &self,
        name: &str,
        argument_at: impl Fn(usize) -> Option<&'k TypeKind>,
    ) -> bool {
        self.conditions.get(name).is_some_and(|condition| {
            condition.always
                || (condition.through.iter())
                    .any(|&at| argument_at(at).is_some_and(|kind| self.holds(kind)))
        })
    }

    /// Whether the generic class `name` is a resource at some instantiation
    /// the program builds, or at every one.
    fn holds_at_some_instantiation(&self, name: &str, program: Program<'_>) -> bool {
        if self
            .conditions
            .get(name)
            .is_some_and(|condition| condition.always)
        {
            return true;
        }
        (program.instantiations.get(name).into_iter().flatten())
            .any(|tuple| self.holds_named(name, |at| tuple.get(at).map(|ty| &ty.kind)))
    }

    /// When an instance of exactly the type `name` defines is a resource: it
    /// runs a hook, or a field it declares or inherits holds one at the
    /// arguments the instance binds. A trait has no instances of its own.
    fn instance_condition(
        &self,
        name: &str,
        definition: &TypeDefinition,
        definitions: &HashMap<String, TypeDefinition>,
    ) -> Condition {
        match definition {
            TypeDefinition::Struct(def) => {
                let scope = def.generics.as_deref().unwrap_or_default();
                (def.fields.iter()).fold(Condition::default(), |condition, (_, ty, _)| {
                    condition.or(self.condition_in(&ty.kind, scope))
                })
            }
            TypeDefinition::Class(_) if has_drop_hook(name, definitions) => Condition::always(),
            TypeDefinition::Class(def) => {
                let scope = def.generics.as_deref().unwrap_or_default();
                let fields = (def.fields.iter())
                    .fold(Condition::default(), |condition, (_, field)| {
                        condition.or(self.condition_in(&field.ty.kind, scope))
                    });
                fields.or(self.base_condition(def, scope))
            }
            TypeDefinition::Enum(_) => Self::condition_if(has_drop_hook(name, definitions)),
            TypeDefinition::Trait(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_) => {
                Condition::default()
            }
        }
    }

    /// When a value held at the type `name` defines is a resource. A value
    /// held at a class may be any class extending it, and one held at a trait
    /// any class implementing it, so either is a resource when such a class
    /// is one at an instantiation the program builds: releasing the value may
    /// then reach that class's hook. A trait declaring the hook is one.
    fn held_condition(
        &self,
        name: &str,
        definition: &TypeDefinition,
        instance: Condition,
        program: Program<'_>,
    ) -> Condition {
        let definitions = program.definitions;
        match definition {
            TypeDefinition::Class(_) => {
                let is_extended_by_a_resource = self
                    .some_class_below(program, |_, sub| sub.base_class.as_deref() == Some(name));
                instance.or(Self::condition_if(is_extended_by_a_resource))
            }
            TypeDefinition::Trait(_) => Self::condition_if(
                trait_declares_drop_hook(name, definitions)
                    || self.some_class_below(program, |class_name, _| {
                        class_implements_trait(class_name, name, definitions)
                    }),
            ),
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => instance,
        }
    }

    /// When the base `def` extends makes an instance of it a resource, the
    /// base's own instances read at the arguments its `extends` clause binds.
    fn base_condition(&self, def: &ClassDefinition, scope: &[GenericDefinition]) -> Condition {
        let Some(base) = def.base_class.as_deref() else {
            return Condition::default();
        };
        let Some(condition) = self.instances.get(base) else {
            return Condition::default();
        };
        let args = def.base_class_args.as_deref().unwrap_or_default();
        self.condition_through(condition, scope, |at| args.get(at).map(|ty| &ty.kind))
    }

    /// Whether some class `is_below` picks is a resource at an instantiation
    /// the program builds, or at every one.
    fn some_class_below(
        &self,
        program: Program<'_>,
        is_below: impl Fn(&str, &ClassDefinition) -> bool,
    ) -> bool {
        program.definitions.iter().any(|(class_name, definition)| {
            matches!(definition, TypeDefinition::Class(class) if is_below(class_name, class))
                && self.holds_at_some_instantiation(class_name, program)
        })
    }

    /// When a member written as `kind` inside a definition declaring the
    /// parameters `scope` makes an instance of that definition a resource.
    fn condition_in(&self, kind: &TypeKind, scope: &[GenericDefinition]) -> Condition {
        if let Some(at) = generic_parameter_name(kind)
            .and_then(|name| scope.iter().position(|generic| generic.name == name))
        {
            let is_bounded_by_a_resource =
                scope[at].bounds.entail(|bound| self.holds(&bound.ty.kind))
                    || matches!(kind, TypeKind::Generic(..) if self.holds(kind));
            return if is_bounded_by_a_resource {
                Condition::always()
            } else {
                Condition::through(at)
            };
        }
        if let TypeKind::Custom(name, args) = kind {
            let Some(condition) = self.conditions.get(name) else {
                return Condition::default();
            };
            let args = args.as_deref().unwrap_or_default();
            return self
                .condition_through(condition, scope, |at| args.get(at).and_then(argument_kind));
        }
        Self::condition_if(self.holds(kind))
    }

    /// `condition`, a named type's, read at the arguments `argument_at`
    /// writes inside a definition declaring the parameters `scope`.
    fn condition_through<'k>(
        &self,
        condition: &Condition,
        scope: &[GenericDefinition],
        argument_at: impl Fn(usize) -> Option<&'k TypeKind>,
    ) -> Condition {
        if condition.always {
            return Condition::always();
        }
        (condition.through.iter())
            .filter_map(|&at| argument_at(at))
            .fold(Condition::default(), |reached, kind| {
                reached.or(self.condition_in(kind, scope))
            })
    }

    fn condition_if(is_resource: bool) -> Condition {
        if is_resource {
            Condition::always()
        } else {
            Condition::default()
        }
    }
}

/// The type a type argument names. A nullable argument names the optional it
/// denotes, which runs no hook of its own, and a value argument names none.
fn argument_kind(arg: &TypeArgument) -> Option<&TypeKind> {
    match &arg.node {
        TypeArgKind::Type(ty, false) => Some(&ty.kind),
        TypeArgKind::Type(_, true) | TypeArgKind::Value(_) => None,
    }
}
