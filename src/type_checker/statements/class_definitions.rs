// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every class of a module is defined — fields, method signatures, hierarchy
//! — before any body of that module is checked. A body written above a class,
//! a free function's or another class's method's, can then read the class's
//! fields and construct it by field name, which it could not if the class
//! were defined only when the body pass reached it.

use std::collections::HashMap;

use crate::ast::{ClassData, Statement, StatementKind};
use crate::type_checker::attributes::DeprecatedKind;
use crate::type_checker::context::Context;
use crate::type_checker::statements::declarations::ClassBodies;
use crate::type_checker::TypeChecker;

/// The classes of one module defined ahead of its bodies, keyed by the id of
/// the statement declaring each. `None` marks a class whose definition was
/// refused — an invalid attribute or name — so the body pass skips it rather
/// than reporting it again.
pub(crate) type DefinedClasses<'a> = HashMap<usize, Option<ClassBodies<'a>>>;

impl TypeChecker {
    /// Defines each class among `statements`, in source order.
    pub(crate) fn define_classes<'a>(
        &mut self,
        statements: impl Iterator<Item = &'a Statement>,
        context: &mut Context,
    ) -> DefinedClasses<'a> {
        let mut defined = DefinedClasses::new();
        for statement in statements {
            if let StatementKind::Class(class_data) = &statement.node {
                let bodies = self.define_class_statement(statement, class_data, context);
                defined.insert(statement.id, bodies);
            }
        }
        defined
    }

    /// Checks `statement` in the body pass: the method bodies of a class
    /// [`define_classes`](Self::define_classes) defined, or the whole of any
    /// other statement.
    pub(crate) fn check_statement_after_definitions(
        &mut self,
        statement: &Statement,
        defined: &DefinedClasses,
        context: &mut Context,
    ) {
        match defined.get(&statement.id) {
            Some(Some(bodies)) => {
                let open_calls = self.open_generic_call_mark();
                self.check_class_bodies(bodies, context);
                self.refuse_open_generic_calls_since(open_calls);
            }
            Some(None) => {}
            None => self.check_statement(statement, context),
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
}
