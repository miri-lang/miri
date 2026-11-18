// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What the program reaches, as the walk over reached bodies decides it: the
//! generic class instances ownership is decided over, each generic body at
//! every set of types the program reaches it at, and whether the body of a
//! slot a call through a trait reads is compiled at an instance.
//!
//! A reached body is named the way its copy is compiled: a top-level
//! function under the module declaring it, a method under the type whose
//! instance runs it. A trait default reached for a class runs at the class
//! its `Self` is bound to, so that class owns its copy. Each copy keeps the
//! chain of bodies that led to it, which explains a refusal and decides
//! nothing.

use super::limits::declared_parameter_name;
use super::{ReachedBindings, SlotVerdicts, Step};
use crate::ast::types::{Type, TypeKind};
use crate::type_checker::context::{class_needs_vtable, Context, MethodInfo, TypeDefinition};
use crate::type_checker::instantiation_limits::instantiation_argument;
use crate::type_checker::instantiation_requirements::{
    free_function_owner, GenericBodyId, SELF_PIN,
};
use crate::type_checker::utils::Instantiations;
use crate::type_checker::{ModuleId, TypeChecker};
use std::collections::HashMap;
use std::rc::Rc;

/// What the walk over reached bodies leaves beside the instances it records.
#[derive(Debug, Default)]
pub(crate) struct ReachedBodies {
    /// Every reached body, at each set of types it is reached at.
    copies: Vec<ReachedCopy>,
    /// The reached bodies with no copy to compile: a member no declaration
    /// gives a body (a field read through its owner), a function the
    /// program's global scope does not name, or a body a parameter of which
    /// the types it is reached at leave unbound.
    unowned: Vec<GenericBodyId>,
    /// Whether the body of a slot is compiled at an instance.
    verdicts: SlotVerdicts,
}

/// The owner a copy of a reached body is compiled under.
#[derive(Debug, Clone, PartialEq)]
pub enum CopyOwner {
    /// The module declaring a top-level function.
    Function(ModuleId),
    /// The type whose instance a method runs at, at that instance's
    /// arguments; none for a type declaring no parameters.
    Type { name: String, arguments: Vec<Type> },
}

/// A generic body at one set of types the program reaches it at.
#[derive(Debug, Clone)]
pub struct ReachedCopy {
    pub owner: CopyOwner,
    /// The function's or the method's name.
    pub name: String,
    /// The type arguments of the body's own parameters, in declaration
    /// order; a method's owner's are its owner's.
    pub arguments: Vec<Type>,
    /// Whether a call through a trait reached the body by reading its slot.
    pub through_slot: bool,
    step: Rc<Step>,
}

impl ReachedCopy {
    /// The declaration this copy is compiled from.
    pub(crate) fn body(&self) -> &GenericBodyId {
        &self.step.body
    }

    /// The type this copy binds the parameter `parameter` to.
    pub(crate) fn bound(&self, parameter: &str) -> Option<&Type> {
        self.step.bound.get(parameter)
    }

    /// The bodies whose sites led to this one, nearest first, each named by
    /// the owner it is declared in and its name; empty for a body a site
    /// outside every generic body reaches.
    pub fn reached_through(&self) -> impl Iterator<Item = (&str, &str)> {
        std::iter::successors(self.step.outer.as_deref(), |step| step.outer.as_deref())
            .map(|step| (step.body.0.as_str(), step.body.1.as_str()))
    }
}

/// The program as the type checker decided it reaches, for the stages that
/// compile it.
pub struct ReachedProgram<'t> {
    instances: &'t Instantiations,
    bodies: &'t ReachedBodies,
    definitions: &'t HashMap<String, TypeDefinition>,
}

impl<'t> ReachedProgram<'t> {
    pub(crate) fn new(
        instances: &'t Instantiations,
        bodies: &'t ReachedBodies,
        definitions: &'t HashMap<String, TypeDefinition>,
    ) -> Self {
        Self {
            instances,
            bodies,
            definitions,
        }
    }

    /// Every generic class instance the program builds or holds, the set
    /// ownership is decided over.
    pub fn instances(&self) -> &'t Instantiations {
        self.instances
    }

    /// Every reached body at each set of types it is reached at, ordered by
    /// owner, name and arguments.
    pub fn copies(&self) -> &'t [ReachedCopy] {
        &self.bodies.copies
    }

    /// The reached bodies no copy could be named for, each once per set of
    /// types it is reached at.
    pub fn unowned(&self) -> &'t [GenericBodyId] {
        &self.bodies.unowned
    }

    /// Every instance among [`Self::instances`] of a class that fills a
    /// vtable: those a vtable may be written for.
    pub fn vtables(&self) -> impl Iterator<Item = (&'t str, &'t [Type])> + '_ {
        (self.instances.iter())
            .filter(|(class, _)| class_needs_vtable(class, self.definitions))
            .flat_map(|(class, tuples)| {
                (tuples.iter()).map(move |arguments| (class.as_str(), arguments.as_slice()))
            })
    }

    /// Whether the body of `method` of `class` is compiled at the instance
    /// `at` where a call through a trait reads its slot; `None` where no
    /// reached call reads it there.
    pub fn slot_verdict(&self, class: &str, method: &str, at: &TypeKind) -> Option<bool> {
        let key = (class.to_string(), method.to_string(), at.clone());
        self.bodies.verdicts.get(&key).copied()
    }

    /// Every slot verdict: the class, the method, the instance, and whether
    /// the body is compiled there.
    pub fn slot_verdicts(&self) -> impl Iterator<Item = (&'t str, &'t str, &'t TypeKind, bool)> {
        (self.bodies.verdicts.iter())
            .map(|((class, method, at), &compiled)| (class.as_str(), method.as_str(), at, compiled))
    }
}

impl TypeChecker {
    /// The copies of every body `reached` lists, named in the program's
    /// global scope `context`, beside `verdicts`.
    pub(super) fn reached_bodies_of(
        &self,
        reached: &ReachedBindings,
        verdicts: SlotVerdicts,
        context: &Context,
    ) -> ReachedBodies {
        let mut copies = Vec::new();
        let mut unowned = Vec::new();
        for step in reached.values().flatten() {
            match self.copy_of(step, context) {
                Some(copy) => copies.push(copy),
                None => unowned.push(step.body.clone()),
            }
        }
        let mut ordered: Vec<_> = (copies.into_iter())
            .map(|copy| (copy_order(&copy), copy))
            .collect();
        ordered.sort_by(|(lhs, _), (rhs, _)| lhs.cmp(rhs));
        ordered.dedup_by(|(lhs, _), (rhs, _)| lhs == rhs);
        let copies = ordered.into_iter().map(|(_, copy)| copy).collect();
        unowned.sort();
        unowned.dedup();
        ReachedBodies {
            copies,
            unowned,
            verdicts,
        }
    }

    /// The copy `step` reaches, under the owner it is compiled for; `None`
    /// when it has none.
    fn copy_of(&self, step: &Rc<Step>, context: &Context) -> Option<ReachedCopy> {
        let (declared_in, name) = &step.body;
        let (owner, parameters) = match self.type_definitions().get(declared_in) {
            Some(definition) => self.method_copy_owner(declared_in, definition, step)?,
            None => self.function_copy_owner(&step.body, context)?,
        };
        Some(ReachedCopy {
            owner,
            name: name.clone(),
            arguments: self.bound_in_order(&parameters, &step.bound)?,
            through_slot: step.through_slot,
            step: Rc::clone(step),
        })
    }

    /// The owner a copy of the method `step` reaches, declared by
    /// `declared_in`, is compiled under, and the method's own parameters: the
    /// class `Self` is bound to, when it is, else the declaring type at the
    /// types its parameters are bound to. `None` for a member `declared_in`
    /// declares no method of.
    fn method_copy_owner(
        &self,
        declared_in: &str,
        definition: &TypeDefinition,
        step: &Step,
    ) -> Option<(CopyOwner, Vec<String>)> {
        let own = (declared_method(definition, &step.body.1)?.generics.iter())
            .map(|generic| generic.name.clone())
            .collect();
        let owner = match step.bound.get(SELF_PIN) {
            Some(receiver) => self.receiver_owner(receiver)?,
            None => CopyOwner::Type {
                name: declared_in.to_string(),
                arguments: self
                    .bound_in_order(&self.owner_parameters(declared_in, definition), &step.bound)?,
            },
        };
        Some((owner, own))
    }

    /// The class instance `receiver` is, as a copy's owner.
    fn receiver_owner(&self, receiver: &Type) -> Option<CopyOwner> {
        let TypeKind::Custom(name, arguments) = &receiver.kind else {
            return None;
        };
        let arguments: Vec<Type> = (arguments.iter().flatten())
            .map(instantiation_argument)
            .collect::<Option<_>>()?;
        Some(CopyOwner::Type {
            name: name.clone(),
            arguments: self.instantiation_arguments(&arguments)?,
        })
    }

    /// The module declaring the top-level function `body` names, as the
    /// program's global scope `context` declares it, and its parameters.
    fn function_copy_owner(
        &self,
        (owner, name): &GenericBodyId,
        context: &Context,
    ) -> Option<(CopyOwner, Vec<String>)> {
        let info = context.resolve_info(name)?;
        let module = ModuleId::checked_as(&info.module);
        if free_function_owner(&module) != *owner {
            return None;
        }
        let TypeKind::Function(function) = &info.ty.kind else {
            return None;
        };
        let parameters = (function.generics.iter().flatten())
            .filter_map(declared_parameter_name)
            .map(str::to_string)
            .collect();
        Some((CopyOwner::Function(module), parameters))
    }

    /// The parameters `definition`, declared as `name`, declares, in order.
    fn owner_parameters(&self, name: &str, definition: &TypeDefinition) -> Vec<String> {
        let declared = match definition {
            TypeDefinition::Class(_) | TypeDefinition::Trait(_) => self.generics_of(name),
            TypeDefinition::Struct(_)
            | TypeDefinition::Enum(_)
            | TypeDefinition::Generic(_)
            | TypeDefinition::Alias(_) => definition.generics().unwrap_or_default(),
        };
        declared
            .iter()
            .map(|generic| generic.name.clone())
            .collect()
    }

    /// The types `bound` binds `parameters` to, in their order, as an
    /// instantiation lists them; `None` when one is unbound or unresolved.
    fn bound_in_order(
        &self,
        parameters: &[String],
        bound: &HashMap<String, Type>,
    ) -> Option<Vec<Type>> {
        let types: Vec<Type> = (parameters.iter())
            .map(|parameter| bound.get(parameter).cloned())
            .collect::<Option<_>>()?;
        self.instantiation_arguments(&types)
    }
}

/// The method `name` `definition` declares itself.
fn declared_method<'d>(definition: &'d TypeDefinition, name: &str) -> Option<&'d MethodInfo> {
    match definition {
        TypeDefinition::Class(class) => class.methods.get(name),
        TypeDefinition::Trait(declared) => declared.methods.get(name),
        TypeDefinition::Enum(declared) => declared.methods.get(name),
        TypeDefinition::Struct(_) | TypeDefinition::Generic(_) | TypeDefinition::Alias(_) => None,
    }
}

/// What orders copies: the owner, the name, then the arguments, each as the
/// program spells it.
fn copy_order(copy: &ReachedCopy) -> (String, String, Vec<String>) {
    let spelled = |types: &[Type]| types.iter().map(|ty| ty.kind.to_string()).collect();
    let owner = match &copy.owner {
        CopyOwner::Function(module) => format!("{module:?}"),
        CopyOwner::Type { name, arguments } => {
            let arguments: Vec<String> = spelled(arguments);
            format!("{name}<{}>", arguments.join(", "))
        }
    };
    (owner, copy.name.clone(), spelled(&copy.arguments))
}
