// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every trait, struct, enum and class of a module is defined — a trait's
//! parents and method signatures, a struct's fields, an enum's variants and
//! method signatures, a class's fields, method signatures and hierarchy —
//! before any body of that module is checked. A body written above the type,
//! a free function's or a method's, can then call the trait's methods, read
//! the type's fields, build its values and match on its variants, which it
//! could not if the type were defined only when the body pass reached it.

use std::collections::HashMap;

use crate::ast::{ClassData, Statement, StatementKind};
use crate::type_checker::attributes::DeprecatedKind;
use crate::type_checker::context::Context;
use crate::type_checker::statements::declarations::{
    ClassBodies, EnumBodies, EnumDeclaration, TraitBodies, TraitDeclaration,
};
use crate::type_checker::TypeChecker;

/// The method bodies of a type defined ahead of the body pass, which the
/// body pass still checks.
pub(crate) enum TypeBodies<'a> {
    Class(ClassBodies<'a>),
    Enum(EnumBodies<'a>),
    Trait(TraitBodies<'a>),
}

/// The types of one module defined ahead of its bodies, keyed by the id of
/// the statement declaring each. `Some` carries the method bodies of a class,
/// enum or trait the body pass still checks; `None` marks a statement the body
/// pass skips: a struct, which has no bodies and was checked in full, or a
/// type whose definition was refused — an invalid attribute or name, or a
/// duplicate — and so must not be reported again.
pub(crate) type DefinedTypes<'a> = HashMap<usize, Option<TypeBodies<'a>>>;

impl TypeChecker {
    /// Defines each trait, then each struct, then each enum, then each
    /// class, among `statements`, each kind in source order. A trait's
    /// signatures and a struct's or enum's depend on nothing but type names,
    /// while a class is checked against the traits it implements and a class
    /// field initializer may construct a struct or an enum.
    pub(crate) fn define_types<'a>(
        &mut self,
        statements: impl Iterator<Item = &'a Statement> + Clone,
        context: &mut Context,
    ) -> DefinedTypes<'a> {
        let mut defined = DefinedTypes::new();
        for statement in statements.clone() {
            if let StatementKind::Trait(..) = &statement.node {
                let bodies = self.define_trait_statement(statement, context);
                defined.insert(statement.id, bodies.map(TypeBodies::Trait));
            }
        }
        for statement in statements.clone() {
            if let StatementKind::Struct(..) = &statement.node {
                self.check_statement(statement, context);
                defined.insert(statement.id, None);
            }
        }
        for statement in statements.clone() {
            if let StatementKind::Enum(..) = &statement.node {
                let bodies = self.define_enum_statement(statement, context);
                defined.insert(statement.id, bodies.map(TypeBodies::Enum));
            }
        }
        for statement in statements {
            if let StatementKind::Class(class_data) = &statement.node {
                let bodies = self.define_class_statement(statement, class_data, context);
                defined.insert(statement.id, bodies.map(TypeBodies::Class));
            }
        }
        defined
    }

    /// Checks `statement` in the body pass: the method bodies of a class or
    /// enum [`define_types`](Self::define_types) defined, nothing for a
    /// struct it checked, or the whole of any other statement.
    pub(crate) fn check_statement_after_definitions(
        &mut self,
        statement: &Statement,
        defined: &DefinedTypes,
        context: &mut Context,
    ) {
        match defined.get(&statement.id) {
            Some(Some(bodies)) => {
                let open_calls = self.open_generic_call_mark();
                match bodies {
                    TypeBodies::Class(bodies) => self.check_class_bodies(bodies, context),
                    TypeBodies::Enum(bodies) => self.check_enum_bodies(bodies, context),
                    TypeBodies::Trait(bodies) => self.check_trait_bodies(bodies, context),
                }
                self.refuse_open_generic_calls_since(open_calls);
            }
            Some(None) => {}
            None => self.check_statement(statement, context),
        }
    }

    /// Checks the `iterator()` body of each class among `statements` before
    /// any other body of the module, so the class each one returns is noted
    /// before a `for` loop anywhere in the module reads it. The body pass then
    /// skips those bodies.
    pub(crate) fn check_iterator_bodies<'a>(
        &mut self,
        statements: impl Iterator<Item = &'a Statement>,
        defined: &DefinedTypes,
        context: &mut Context,
    ) {
        for statement in statements {
            let Some(Some(TypeBodies::Class(bodies))) = defined.get(&statement.id) else {
                continue;
            };
            let open_calls = self.open_generic_call_mark();
            self.check_class_iterator_body(bodies, context);
            self.refuse_open_generic_calls_since(open_calls);
        }
    }

    /// Defines one class under the attribute check and open-call refusal
    /// `check_statement` applies to every statement.
    fn define_class_statement<'a>(
        &mut self,
        statement: &'a Statement,
        class_data: &'a ClassData,
        context: &mut Context,
    ) -> Option<ClassBodies<'a>> {
        if !self.check_declaration_attributes(statement) {
            return None;
        }
        self.collect_deprecated_type(
            &class_data.name,
            DeprecatedKind::Class,
            &class_data.attributes,
        );
        let open_calls = self.open_generic_call_mark();
        let bodies = self.define_class(class_data, context, statement.span);
        self.refuse_open_generic_calls_since(open_calls);
        bodies
    }

    /// Defines one trait under the attribute check and open-call refusal
    /// `check_statement` applies to every statement.
    fn define_trait_statement<'a>(
        &mut self,
        statement: &'a Statement,
        context: &mut Context,
    ) -> Option<TraitBodies<'a>> {
        let StatementKind::Trait(name, generics, parent_traits, body, visibility) = &statement.node
        else {
            return None;
        };
        if !self.check_declaration_attributes(statement) {
            return None;
        }
        let open_calls = self.open_generic_call_mark();
        let declaration = TraitDeclaration {
            name,
            generics,
            parent_traits,
            body,
            visibility,
            span: statement.span,
        };
        let bodies = self.define_trait(declaration, context);
        self.refuse_open_generic_calls_since(open_calls);
        bodies
    }

    /// Defines one enum under the attribute check, deprecation note and
    /// open-call refusal `check_statement` applies to every statement.
    fn define_enum_statement<'a>(
        &mut self,
        statement: &'a Statement,
        context: &mut Context,
    ) -> Option<EnumBodies<'a>> {
        let StatementKind::Enum(name, generics, variants, methods, visibility, attributes) =
            &statement.node
        else {
            return None;
        };
        if !self.check_declaration_attributes(statement) {
            return None;
        }
        self.collect_deprecated_type(name, DeprecatedKind::Enum, attributes);
        let open_calls = self.open_generic_call_mark();
        let declaration = EnumDeclaration {
            name,
            generics,
            variants,
            methods,
            attributes,
            visibility,
        };
        let bodies = self.define_enum(declaration, context);
        self.refuse_open_generic_calls_since(open_calls);
        bodies
    }
}
