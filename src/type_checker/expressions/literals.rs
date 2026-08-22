// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Expression type inference for the type checker.
//!
//! This module implements type inference for all expression kinds in Miri.
//! The main entry point is [`TypeChecker::infer_expression`], which dispatches
//! to specialized inference methods based on the expression kind.
//!
//! # Supported Expressions
//!
//! ## Literals
//! - Integer, float, string, boolean, and none literals
//!
//! ## Operators
//! - Binary: arithmetic (`+`, `-`, `*`, `/`, `%`), comparison (`<`, `>`, `==`, etc.)
//! - Logical: `and`, `or`
//! - Unary: `-`, `+`, `not`, `~`, `await`
//!
//! ## Collections
//! - Lists: `[1, 2, 3]` → `List<int>`
//! - Maps: `{"a": 1}` → `Map<string, int>`
//! - Sets: `{1, 2, 3}` → `Set<int>`
//! - Tuples: `(1, "a")` → `(int, string)`
//! - Ranges: `1..10` → `Range<int>`
//!
//! ## Access
//! - Member access: `obj.field`
//! - Index access: `list[0]`, `map["key"]`
//!
//! ## Functions
//! - Function calls with generic type inference
//! - Lambda expressions with type inference
//! - Method calls on objects
//!
//! ## Control Flow
//! - Conditional expressions: `x if cond else y`
//! - Match expressions with pattern matching
//!
//! ## Types
//! - Struct instantiation: `Point { x: 1, y: 2 }`
//! - Enum variant construction: `Ok(value)`, `Err(error)`
//! - Generic type instantiation

use crate::ast::factory as ast_factory;
use crate::ast::factory::make_type;
use crate::ast::types::{Type, TypeKind, REGEX_TYPE_NAME};
use crate::ast::*;
use crate::error::syntax::Span;
use crate::error::type_error::{TypeError, TypeErrorKind};
use crate::type_checker::context::Context;
use crate::type_checker::TypeChecker;

/// Maximum nesting depth for enum type interpolation checks to prevent
/// stack overflow on deeply-nested acyclic enum chains.
const MAX_ENUM_INTERPOLATE_DEPTH: usize = 64;

// Thread-local tracking of depth and visited enums during enum interpolation checks.
// Both depth and visited set are shared across an entire top-level check to properly
// detect cycles and enforce the depth bound, even when can_interpolate() delegates to
// nested check_enum_can_interpolate() calls.
thread_local! {
    static ENUM_INTERPOLATE_STATE: std::cell::RefCell<EnumInterpolateState> =
        std::cell::RefCell::new(EnumInterpolateState {
            depth: 0,
            visited: std::collections::HashSet::new(),
        });
}

struct EnumInterpolateState {
    depth: usize,
    visited: std::collections::HashSet<String>,
}

impl TypeChecker {
    /// Rejects an integer literal whose value does not fit the default `int`
    /// type (`i64`), which would otherwise be silently truncated to a garbage
    /// value during MIR lowering and codegen.
    ///
    /// A literal directly under a unary negation may reach `|i64::MIN|`
    /// (`i64::MAX + 1`), since `i64::MIN` can only be spelled
    /// `-9223372036854775808`; a bare positive literal may reach only `i64::MAX`.
    pub(crate) fn check_integer_literal_range(
        &mut self,
        lit: &Literal,
        expr_id: usize,
        span: Span,
    ) {
        let Literal::Integer(int_lit) = lit else {
            return;
        };
        // A literal explicitly declared with a wider integer type keeps its full
        // i128-representable range (the parser already rejects anything larger).
        if self.wide_typed_int_literals.contains(&expr_id) {
            return;
        }
        let value = int_lit.to_i128();
        let max = if self.negated_int_literals.contains(&expr_id) {
            i64::MAX as i128 + 1
        } else {
            i64::MAX as i128
        };
        if value > max {
            self.report_error(
                format!(
                    "Integer literal '{}' is out of range for the default int type (i64, max {})",
                    value,
                    i64::MAX
                ),
                span,
            );
        }
    }

    /// Validates a regex literal by checking flags and compiling the pattern.
    pub(crate) fn check_regex_literal(&mut self, lit: &Literal, span: Span, context: &Context) {
        let Literal::Regex(token) = lit else {
            return;
        };

        if context.in_gpu_function {
            self.report_error(
                "Regex literals cannot be used inside a GPU function; use Regex.compile() at host level and pass it as a parameter"
                    .to_string(),
                span,
            );
            return;
        }

        if token.global {
            let error = TypeError::new(
                TypeErrorKind::InvalidRegexLiteral {
                    reason: "Regex literal does not support the 'g' flag; use find_all() for global matching"
                        .to_string(),
                },
                span,
            );
            self.report_typed_error(error);
            return;
        }

        let pattern = build_regex_pattern(
            &token.body,
            token.ignore_case,
            token.multiline,
            token.dot_all,
            token.unicode,
        );

        if let Err(e) = regex::Regex::new(&pattern) {
            let error = TypeError::new(
                TypeErrorKind::InvalidRegexLiteral {
                    reason: format!("{}", e),
                },
                span,
            );
            self.report_typed_error(error);
        }
    }

    /// A float literal is width-less in the source, so — like an integer
    /// literal, which is always `Int` here — it infers as the target's default
    /// float width and takes a narrower one only from a context that declares
    /// it. The `F32` spelling appears only on literals the compiler synthesizes
    /// itself (a reduce identity, a frame uniform), which choose their width
    /// deliberately and keep it.
    pub(crate) fn infer_literal(&self, lit: &Literal, context: &Context) -> Type {
        match lit {
            Literal::Integer(_) => ast_factory::make_type(TypeKind::Int),
            Literal::Float(f) => match f {
                FloatLiteral::F32(_) => ast_factory::make_type(TypeKind::F32),
                FloatLiteral::F64(_) => ast_factory::make_type(
                    crate::type_checker::float_literals::default_float_literal_width(context),
                ),
            },
            Literal::Boolean(_) => ast_factory::make_type(TypeKind::Boolean),
            Literal::String(_) => ast_factory::make_type(TypeKind::String),
            Literal::Identifier(_) => ast_factory::make_type(TypeKind::Identifier),
            Literal::Regex(_) => {
                ast_factory::make_type(TypeKind::Custom(REGEX_TYPE_NAME.into(), None))
            }
            Literal::None => ast_factory::make_type(TypeKind::Option(Box::new(
                ast_factory::make_type(TypeKind::Void),
            ))),
        }
    }

    pub(crate) fn infer_formatted_string(
        &mut self,
        parts: &[Expression],
        context: &mut Context,
    ) -> Type {
        for part in parts {
            let part_type = self.infer_expression(part, context);
            // Literal string segments are always fine; only validate interpolated expressions.
            if !matches!(&part.node, ExpressionKind::Literal(Literal::String(_)))
                && !self.can_interpolate(&part_type.kind)
            {
                self.report_error(
                    format!(
                        "Type '{}' cannot be used in string interpolation",
                        part_type
                    ),
                    part.span,
                );
            }
        }
        make_type(TypeKind::String)
    }

    /// Returns `true` if a value of this type can be converted to a string
    /// for use in formatted string interpolation.
    ///
    /// Supports scalars (int, float, bool, string), Option<T>, Result<T, E>,
    /// and user-defined enums. For Option and Result, the payloads must
    /// recursively support interpolation. For user enums, all variant payloads
    /// must recursively support interpolation.
    ///
    /// Returns `false` for collection types (List, Map, Set) and unresolved
    /// generics, which are not formattable.
    pub(crate) fn can_interpolate(&self, kind: &TypeKind) -> bool {
        match kind {
            // Scalar types that have simple to-string conversions
            TypeKind::String
            | TypeKind::Boolean
            | TypeKind::Int
            | TypeKind::I8
            | TypeKind::I16
            | TypeKind::I32
            | TypeKind::I64
            | TypeKind::I128
            | TypeKind::U8
            | TypeKind::U16
            | TypeKind::U32
            | TypeKind::U64
            | TypeKind::U128
            | TypeKind::Float
            | TypeKind::F32
            | TypeKind::F64
            | TypeKind::Error => true,

            // Option<T>: T must be interpolatable
            TypeKind::Option(inner) => self.can_interpolate(&inner.kind),

            // Result<T, E> (parser form): both T and E must be interpolatable
            // This is kept for defensive handling; the type checker normalizes
            // Result to Custom form, but both forms may reach lowering.
            TypeKind::Result(ok_expr, err_expr) => {
                // Recursively check both type arguments' formatability.
                // Expression-typed arguments are resolved by pattern-matching on
                // their structure (Type nodes, Identifiers) rather than a full lookup,
                // so this is safe without circularity.
                use crate::ast::expression::ExpressionKind;
                let check_expr = |expr: &Expression| -> bool {
                    match &expr.node {
                        ExpressionKind::Type(t, _) => self.can_interpolate(&t.kind),
                        ExpressionKind::Identifier(name, _) => {
                            // Look up primitive types by name
                            if let Some(prim_kind) = crate::ast::types::primitive_type_kind(name) {
                                self.can_interpolate(&prim_kind)
                            } else if self.type_definitions().contains_key(name) {
                                // Custom type: check as enum
                                self.check_enum_can_interpolate(name, None)
                            } else {
                                false
                            }
                        }
                        _ => false,
                    }
                };
                check_expr(ok_expr) && check_expr(err_expr)
            }

            // Custom type: check if it's an enum and all payloads are interpolatable
            TypeKind::Custom(name, type_args) => {
                self.check_enum_can_interpolate(name, type_args.as_deref())
            }

            // Unresolved generics cannot be formatted
            TypeKind::Generic(_, _, _) => false,

            // Collections are not formattable
            TypeKind::List(_)
            | TypeKind::Map(_, _)
            | TypeKind::Set(_)
            | TypeKind::Array(_, _)
            | TypeKind::Tuple(_)
            | TypeKind::Identifier
            | TypeKind::Void => false,

            // Future types are not formattable
            TypeKind::Future(_) => false,

            // Meta and Linear are not formattable
            TypeKind::Meta(_)
            | TypeKind::Linear(_)
            | TypeKind::RawPtr
            | TypeKind::F16
            | TypeKind::Function(_) => false,
        }
    }

    /// Check if an enum type can be interpolated.
    ///
    /// An enum can be interpolated if all of its variant payloads
    /// can be interpolated. This method uses a cycle guard to detect
    /// direct and mutual recursion in enum definitions. A self-referential
    /// enum like `enum Wrapper: Wrap(Wrapper)` or mutually-recursive enums
    /// like `enum A: X(B)` and `enum B: Y(A)` cannot be interpolated
    /// because rendering would require infinite recursion.
    ///
    /// Collection types (List, Map, Set, Array) are never formattable,
    /// which also blocks indirect recursion like `enum Json: Value([Json]?)`.
    ///
    /// A depth bound prevents stack overflow on deeply-nested acyclic chains,
    /// where the cycle guard alone cannot help.
    fn check_enum_can_interpolate(&self, name: &str, type_args: Option<&[Expression]>) -> bool {
        ENUM_INTERPOLATE_STATE.with(|state| {
            let mut s = state.borrow_mut();
            let is_top_level = s.depth == 0;

            // At the top level, start fresh. At nested levels, reuse the same
            // state to properly detect cycles across the entire chain.
            if is_top_level {
                s.visited.clear();
            }

            drop(s);
            let result = self.check_enum_can_interpolate_impl(name, type_args);

            // Reset state at the top level.
            if is_top_level {
                let mut s = state.borrow_mut();
                s.visited.clear();
                s.depth = 0;
            }

            result
        })
    }

    fn check_enum_can_interpolate_impl(
        &self,
        name: &str,
        type_args: Option<&[Expression]>,
    ) -> bool {
        let should_return_early = ENUM_INTERPOLATE_STATE.with(|state| {
            let mut s = state.borrow_mut();

            // Check and increment depth.
            if s.depth >= MAX_ENUM_INTERPOLATE_DEPTH {
                return true; // Signal to return false
            }
            s.depth += 1;

            // If already visited, we've detected a cycle.
            if s.visited.contains(name) {
                s.depth -= 1;
                return true; // Signal to return false
            }

            false // Signal to continue
        });

        if should_return_early {
            return false;
        }

        let Some(crate::type_checker::context::TypeDefinition::Enum(enum_def)) =
            self.type_table.global_type_definitions.get(name)
        else {
            ENUM_INTERPOLATE_STATE.with(|state| {
                state.borrow_mut().depth -= 1;
            });
            return false;
        };

        // Mark this enum as visited before checking its payloads.
        ENUM_INTERPOLATE_STATE.with(|state| {
            state.borrow_mut().visited.insert(name.to_string());
        });

        // Check that all variant payloads are formattable.
        let result = enum_def.variants.values().all(|payload_types| {
            payload_types.iter().all(|payload_ty| {
                // Substitute generic type parameters if this is a generic enum.
                let concrete_kind = crate::type_checker::generics::substitute_generic_field_kind(
                    &payload_ty.kind,
                    type_args,
                    enum_def.generics.as_ref(),
                );

                // Recursively check if the concrete type is interpolatable.
                // Depth and visited set are maintained via thread-local state.
                match &concrete_kind {
                    crate::ast::types::TypeKind::Custom(other_name, other_args) => {
                        self.check_enum_can_interpolate_impl(other_name, other_args.as_deref())
                    }
                    _ => self.can_interpolate(&concrete_kind),
                }
            })
        });

        // Remove this enum from visited before returning.
        // This allows it to be visited in different recursive branches.
        ENUM_INTERPOLATE_STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.visited.remove(name);
            s.depth -= 1;
        });

        result
    }
}

/// Builds a regex pattern string from a body and flags.
///
/// Maps regex flags to inline prefixes: `i` → `(?i)`, `m` → `(?m)`, `s` → `(?s)`, `u` → `(?u)`.
/// Combined flags are merged into a single group (e.g., `(?im)` for both ignore_case and multiline).
/// This function is shared by the type checker (to validate patterns) and MIR lowering (to construct Regex values).
pub(crate) fn build_regex_pattern(
    body: &str,
    ignore_case: bool,
    multiline: bool,
    dot_all: bool,
    unicode: bool,
) -> String {
    let mut prefix = String::new();

    if ignore_case {
        prefix.push('i');
    }
    if multiline {
        prefix.push('m');
    }
    if dot_all {
        prefix.push('s');
    }
    if unicode {
        prefix.push('u');
    }

    if prefix.is_empty() {
        body.to_string()
    } else {
        format!("(?{}){}", prefix, body)
    }
}
