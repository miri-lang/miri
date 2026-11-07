// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The rules a generic method meets because every call to one is static.
//!
//! A method declaring type parameters of its own, `fn m<T>(x T) T`, is
//! compiled once per type argument its calls bind, and no vtable slot holds
//! it: a slot holds one body. A call therefore names the copy of the body
//! the receiver's static type reaches, so:
//!
//! - a class redeclaring a method a class it extends has as a generic one,
//!   or declaring a generic method over one it inherits, is refused: a
//!   receiver typed as the base would run the base's body, not the override;
//! - a class leaves no generic method without a body, since no subclass
//!   could supply one; only a trait declares one without a body, and each
//!   implementing class's copy is reached through the class itself;
//! - device code declares no generic method, since it is compiled once.
//! - a constructor or a drop hook declares no type parameters of its own,
//!   since building or releasing an instance binds none.

use std::collections::{BTreeMap, HashMap};

use crate::ast::bounds::Bounds;
use crate::ast::factory::make_type;
use crate::ast::implicit_methods::CONSTRUCTION_METHOD_NAMES;
use crate::ast::types::{Type, TypeKind};
use crate::ast::{FunctionDeclarationData, Statement, StatementKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;
use crate::type_checker::context::{
    resolve_method_source, GenericDefinition, MethodInfo, MethodSource,
};
use crate::type_checker::diagnostics::shown_type_name;
use crate::type_checker::TypeChecker;

/// The help a refused generic method in device code carries.
pub(crate) const DEVICE_GENERIC_METHOD_HELP: &str = "use a free generic function";

/// A method of a class meeting a method of the same name a class it extends
/// has, at least one of the two generic.
struct GenericOverride {
    method: String,
    /// The class the inherited method belongs to: the one declaring it, or
    /// the one a trait default reaches it through.
    base: String,
    /// Whether the inherited method is the generic one.
    base_is_generic: bool,
}

impl TypeChecker {
    /// Refuse each method of the class `name` that meets a method of the
    /// same name `base_name` or a class above it has, when either is generic.
    /// Reported at the redeclaring method. Constructors are not inherited,
    /// and two static methods are each called through the class that names
    /// them, so neither pair overrides.
    pub(super) fn refuse_generic_method_overrides(
        &mut self,
        name: &str,
        base_name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        method_statements: &[&Statement],
        fallback: Span,
    ) {
        let overrides: Vec<GenericOverride> = methods
            .iter()
            .filter(|(_, own)| !own.is_constructor)
            .filter_map(|(method, own)| self.generic_override(base_name, method, own))
            .collect();
        for found in overrides {
            let span = method_statement_span(method_statements, &found.method).unwrap_or(fallback);
            self.report_generic_override(name, &found, span);
        }
    }

    /// The inherited method `method` of `base_name` the class's own `own`
    /// meets, when one of the two is generic.
    fn generic_override(
        &self,
        base_name: &str,
        method: &str,
        own: &MethodInfo,
    ) -> Option<GenericOverride> {
        let inherited =
            resolve_method_source(&self.type_table.global_type_definitions, base_name, method)?;
        let base = match inherited {
            MethodSource::Declared { class, .. } | MethodSource::AbstractOnly { class, .. } => {
                class
            }
            MethodSource::Default(default) => default.class_level,
        };
        let inherited_info = inherited.info();
        let base_is_generic = !inherited_info.generics.is_empty();
        let either_generic = base_is_generic || !own.generics.is_empty();
        let both_static = own.is_static && inherited_info.is_static;
        (either_generic && !both_static).then(|| GenericOverride {
            method: method.to_string(),
            base: base.to_string(),
            base_is_generic,
        })
    }

    fn report_generic_override(&mut self, name: &str, found: &GenericOverride, span: Span) {
        let class = shown_type_name(name);
        let base = shown_type_name(&found.base);
        let method = &found.method;
        let (message, help) = if found.base_is_generic {
            (
                format!(
                    "`{method}` of `{class}` overrides the generic method `{method}` of \
                     `{base}`; every call to a generic method runs the body its receiver's \
                     type declares, so a receiver typed as `{base}` would never run it"
                ),
                format!(
                    "a generic method cannot be overridden; move the part that varies into a \
                     non-generic method that `{class}` overrides, or declare the type \
                     parameter on the class"
                ),
            )
        } else {
            (
                format!(
                    "the generic method `{method}` of `{class}` overrides `{method}` of \
                     `{base}`; a generic method takes no vtable slot, so a receiver typed as \
                     `{base}` would never run it"
                ),
                format!(
                    "a generic method cannot override another; give `{method}` another name, \
                     or declare the type parameter on the class"
                ),
            )
        };
        self.report_error_with_help(
            DiagnosticCode::TypGenericMethodOverridden,
            message,
            span,
            help,
        );
    }

    /// Refuse each generic method of the class `name` declared without a
    /// body: a generic method cannot be overridden, so no subclass could
    /// supply one.
    pub(super) fn refuse_bodiless_generic_methods(
        &mut self,
        name: &str,
        methods: &BTreeMap<String, MethodInfo>,
        method_statements: &[&Statement],
        fallback: Span,
    ) {
        let class = shown_type_name(name);
        let bodiless = methods
            .iter()
            .filter(|(_, info)| info.is_abstract && !info.generics.is_empty())
            .map(|(method, _)| method);
        for method in bodiless {
            let span = method_statement_span(method_statements, method).unwrap_or(fallback);
            self.report_error_with_help(
                DiagnosticCode::TypGenericMethodOverridden,
                format!(
                    "the generic method `{method}` of `{class}` has no body; only a trait may \
                     declare a generic method without one"
                ),
                span,
                format!(
                    "give `{method}` a body: a generic method cannot be overridden, so no \
                     subclass could supply one"
                ),
            );
        }
    }

    /// Refuse the abstract class `name` leaving each `(method, trait)` of
    /// `open`, a generic method the trait declares with no default, without a
    /// body: a call through a receiver typed as the class names the class's
    /// own copy, which no subclass can supply.
    pub(super) fn refuse_open_generic_trait_methods(
        &mut self,
        name: &str,
        open: &[(String, String)],
        span: Span,
    ) {
        for (method, trait_name) in open {
            self.refuse_open_generic_trait_method(name, method, trait_name, span);
        }
    }

    fn refuse_open_generic_trait_method(
        &mut self,
        name: &str,
        method: &str,
        trait_name: &str,
        span: Span,
    ) {
        let class = shown_type_name(name);
        let shown_trait = shown_type_name(trait_name);
        self.report_error_with_help(
            DiagnosticCode::TypGenericMethodOverridden,
            format!(
                "the abstract class `{class}` leaves the generic method `{method}` of the trait \
                 `{shown_trait}` without a body; only a trait may declare a generic method \
                 without one"
            ),
            span,
            format!(
                "give `{method}` a body in `{class}`; a generic method is compiled per call, so \
                 a subclass cannot supply it later"
            ),
        );
    }

    /// Refuse `decl`, a constructor or a drop hook of a type of `owner_kind`
    /// (`class`, `enum`), when it declares type parameters of its own:
    /// building or releasing an instance runs it, and binds no type argument
    /// a copy could be compiled at.
    pub(crate) fn refuse_generic_construction_method(
        &mut self,
        decl: &FunctionDeclarationData,
        span: Span,
        owner_kind: &str,
    ) {
        let is_construction = CONSTRUCTION_METHOD_NAMES.contains(&decl.name.as_str());
        if !is_construction || !decl.declares_type_parameters() {
            return;
        }
        let method = &decl.name;
        self.report_error_with_help(
            DiagnosticCode::TypGenericMethodOverridden,
            format!(
                "`{method}` declares type parameters of its own, but it runs when an instance \
                 is built or released, which binds no type arguments"
            ),
            span,
            format!("declare the type parameter on the {owner_kind}"),
        );
    }

    /// Refuse `decl`, a method declared as device code (`gpu fn`), when it
    /// declares type parameters of its own: device code is compiled once.
    pub(crate) fn refuse_generic_device_method(
        &mut self,
        decl: &FunctionDeclarationData,
        span: Span,
    ) {
        if !decl.properties.is_gpu || !decl.declares_type_parameters() {
            return;
        }
        self.report_error_with_help(
            DiagnosticCode::TarGpuCodeRestriction,
            format!(
                "the generic method `{}` is declared as device code, which is compiled once \
                 rather than per type argument",
                decl.name
            ),
            span,
            DEVICE_GENERIC_METHOD_HELP.to_string(),
        );
    }
}

impl TypeChecker {
    /// The class method `own`'s type parameters, each mapped to the trait
    /// method `required`'s at the same position, so `fn m<U>(x U) U` reads as
    /// `fn m<T>(x T) T`. `substitute` reads a type of the trait at the
    /// arguments the class implements it at. `None` when the two declare a
    /// different number of parameters, or a parameter is bounded otherwise
    /// once renamed.
    pub(super) fn trait_parameter_renaming(
        &self,
        required: &MethodInfo,
        own: &MethodInfo,
        substitute: &impl Fn(&Type) -> Type,
    ) -> Option<HashMap<String, Type>> {
        if required.generics.len() != own.generics.len() {
            return None;
        }
        let renaming: HashMap<String, Type> = own
            .generics
            .iter()
            .zip(&required.generics)
            .map(|(own, required)| {
                let at_trait = TypeKind::Generic(required.name.clone(), required.bounds.clone());
                (own.name.clone(), make_type(at_trait))
            })
            .collect();
        let bounds_agree = own
            .generics
            .iter()
            .zip(&required.generics)
            .all(|(own, required)| {
                same_bounds(
                    &own.bounds
                        .map_types(|ty| self.substitute_type(ty, &renaming)),
                    &required.bounds.map_types(substitute),
                )
            });
        bounds_agree.then_some(renaming)
    }
}

/// Whether `left` and `right` declare the same bounds, in any order.
fn same_bounds(left: &Bounds, right: &Bounds) -> bool {
    let holds_all = |of: &Bounds, among: &Bounds| {
        of.iter().all(|bound| {
            among
                .iter()
                .any(|other| other.kind == bound.kind && other.ty.kind == bound.ty.kind)
        })
    };
    left.len() == right.len() && holds_all(left, right) && holds_all(right, left)
}

/// The signature `method` spells under the name `name`, its types read
/// through `substitute`: `fn pick<T>(x: T) -> T`.
pub(crate) fn spelled_signature(
    name: &str,
    method: &MethodInfo,
    substitute: &impl Fn(&Type) -> Type,
) -> String {
    let params: Vec<String> = (method.params.iter())
        .map(|(param, ty)| format!("{param}: {}", substitute(ty)))
        .collect();
    format!(
        "fn {name}{}({}) -> {}",
        generic_parameter_list(&method.generics, substitute),
        params.join(", "),
        substitute(&method.return_type)
    )
}

/// The type parameter list `generics` spells in a signature, `<T, U
/// implements Comparable>`, each bound read through `substitute`; empty when
/// there are none.
fn generic_parameter_list(
    generics: &[GenericDefinition],
    substitute: &impl Fn(&Type) -> Type,
) -> String {
    if generics.is_empty() {
        return String::new();
    }
    let spelled: Vec<String> = generics
        .iter()
        .map(|generic| {
            if generic.bounds.is_empty() {
                generic.name.clone()
            } else {
                format!("{} {}", generic.name, generic.bounds.map_types(substitute))
            }
        })
        .collect();
    format!("<{}>", spelled.join(", "))
}

/// The span of the declaration of `method` among `method_statements`.
fn method_statement_span(method_statements: &[&Statement], method: &str) -> Option<Span> {
    method_statements
        .iter()
        .find(|stmt| {
            matches!(&stmt.node, StatementKind::FunctionDeclaration(decl) if decl.name == method)
        })
        .map(|stmt| stmt.span)
}
