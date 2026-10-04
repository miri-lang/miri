// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Type compatibility checking for the type checker.
//!
//! This module handles determining whether types are compatible for
//! assignments, function calls, and operations. It includes support for:
//! - Structural type equality
//! - Option type compatibility
//! - Numeric type widening
//! - Subtyping (inheritance, interfaces, mixins)
//! - Generic type constraints

use super::context::{Context, TypeDefinition};
use super::TypeChecker;
use crate::ast::expression::ExpressionKind;
use crate::ast::types::{
    BuiltinCollectionKind, Type, TypeDeclarationKind, TypeKind, STRING_TYPE_NAME,
};

impl TypeChecker {
    /// Checks if two types are compatible for assignment or operation.
    ///
    /// This function handles:
    /// - Exact type equality
    /// - Option type compatibility (`T` is compatible with `T?`, `None` is compatible with `T?`)
    /// - Numeric type compatibility (literals, widening)
    /// - Inheritance/Interface implementation (via `is_subtype`)
    /// - Generic type constraints
    pub(crate) fn are_compatible(&self, t1: &Type, t2: &Type, context: &Context) -> bool {
        // Fast path: the same type, wherever each was written.
        if t1.kind == t2.kind {
            return true;
        }

        // Suppress cascade errors: if either type is Error (from a prior error),
        // treat them as compatible to avoid noisy secondary diagnostics.
        if matches!(t1.kind, TypeKind::Error) || matches!(t2.kind, TypeKind::Error) {
            return true;
        }

        if let Some(parameter) = Self::written_parameter(t1, context) {
            return self.are_compatible(&parameter, t2, context);
        }
        if let Some(parameter) = Self::written_parameter(t2, context) {
            return self.are_compatible(t1, &parameter, context);
        }

        // Handle optional types
        if let Some(result) = self.check_option_compatibility(t1, t2, context) {
            return result;
        }

        // Handle numeric compatibility
        if let Some(result) = self.check_numeric_compatibility(t1, t2) {
            return result;
        }

        // TypeKind::String and Custom("String") are the same type
        if self.is_string_type(t1) && self.is_string_type(t2) {
            return true;
        }

        // Handle custom types (inheritance, interfaces)
        if let Some(result) = self.check_custom_type_compatibility(t1, t2, context) {
            return result;
        }

        // Handle collection types
        if let Some(result) = self.check_collection_compatibility(t1, t2, context) {
            return result;
        }

        // Handle tuple types
        if let Some(result) = self.check_tuple_compatibility(t1, t2, context) {
            return result;
        }

        // Handle function types
        if let Some(result) = self.check_function_compatibility(t1, t2, context) {
            return result;
        }

        // Handle generic types
        if let Some(result) = self.check_generic_compatibility(t1, t2, context) {
            return result;
        }

        // Default: structural equality
        t1 == t2
    }

    /// Checks option type compatibility.
    fn check_option_compatibility(&self, t1: &Type, t2: &Type, context: &Context) -> Option<bool> {
        if let TypeKind::Option(inner) = &t1.kind {
            // Option(T) accepts T or None
            if let TypeKind::Option(inner2) = &t2.kind {
                if matches!(inner2.kind, TypeKind::Void) {
                    return Some(true); // None is compatible with any optional
                }
                return Some(self.are_compatible(inner, inner2, context));
            }
            // Also accepts non-optional T
            return Some(self.are_compatible(inner, t2, context));
        }

        // Non-optional type cannot accept optional — but defer when t1 is a
        // generic parameter so the generic compatibility check can handle it.
        if let TypeKind::Option(_) = &t2.kind {
            if matches!(t1.kind, TypeKind::Generic(..)) {
                return None;
            }
            return Some(false);
        }

        None
    }

    /// Checks numeric type compatibility including literal widening.
    ///
    /// A number is stored where its value always fits: the destination holds
    /// every value of the source on every target. A literal took the
    /// destination's type where it was written, so it is judged at that type.
    fn check_numeric_compatibility(&self, t1: &Type, t2: &Type) -> Option<bool> {
        let numbers = |ty: &Type| {
            crate::ast::types::scalar_width(&ty.kind).is_some()
                && !matches!(ty.kind, TypeKind::Boolean)
        };
        if numbers(t1) && numbers(t2) {
            return Some(crate::ast::types::holds_every_value_of(&t1.kind, &t2.kind));
        }
        // Int literal compatible with any integer type
        if matches!(t2.kind, TypeKind::Int) && self.is_integer(t1) {
            return Some(true);
        }

        // Float literal compatible with any float type
        if matches!(t2.kind, TypeKind::Float)
            && matches!(t1.kind, TypeKind::F16 | TypeKind::F32 | TypeKind::F64)
        {
            return Some(true);
        }

        // F16/F32/F64 compatible with Float variable
        if matches!(t1.kind, TypeKind::Float)
            && matches!(t2.kind, TypeKind::F16 | TypeKind::F32 | TypeKind::F64)
        {
            return Some(true);
        }

        // Integer widening: smaller to larger
        if self.is_integer(t1) && self.is_integer(t2) {
            if let (Some(s1), Some(s2)) = (self.get_integer_size(t1), self.get_integer_size(t2)) {
                if s1 >= s2 {
                    return Some(true);
                }
            }
        }

        // Float widening: F32 to F64
        if matches!(t1.kind, TypeKind::F64) && matches!(t2.kind, TypeKind::F32) {
            return Some(true);
        }

        None
    }

    /// Checks custom type compatibility (inheritance, interfaces).
    fn check_custom_type_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        if let (TypeKind::Custom(n1, args1), TypeKind::Custom(n2, args2)) = (&t1.kind, &t2.kind) {
            // Delegate builtin collection types to check_collection_compatibility,
            // which has dedicated size/element-type logic (e.g. Array size matching).
            if BuiltinCollectionKind::from_name(n1.as_str()).is_some()
                || BuiltinCollectionKind::from_name(n2.as_str()).is_some()
            {
                return None;
            }

            if n1 == n2 {
                // Result<T, E> special case: void on either side is a wildcard
                // (Ok(x) infers Result<T, void> and Err(e) infers Result<void, E>).
                if n1 == "Result" {
                    return Some(self.check_result_args_compatible(args1, args2, context));
                }
                // A generic enum spelled with no arguments is one whose
                // instantiation is not yet known: a variant carrying no payload
                // gives nothing to infer the arguments from. The slot it is
                // written into names them, so the two agree — where the variant
                // does carry a payload, the arguments come with it and are
                // compared below like any others.
                if self.enum_instantiation_is_open(n1, args1, args2, context) {
                    return Some(true);
                }
                // Same type name - check generic arguments
                return Some(self.check_generic_args_compatible(args1, args2, context));
            }

            // Check subtyping relationship
            if self.is_subtype(n2, n1) {
                return Some(self.inherited_arguments_compatible(n1, args1, n2, args2, context));
            }
        }
        None
    }

    /// Whether `sub<sub_args>` reaches its supertype `sup` at the arguments
    /// `sup_args` spells, read through the clauses on the way up: a class
    /// implementing `Op<Foo>` is no `Op<String>`.
    ///
    /// A parameter the clauses leave to nothing `sub_args` binds, or a path the
    /// clauses do not spell (a mixin), gives no argument to compare, and an
    /// argument that cannot be read is no argument the value was built at: the
    /// two are refused rather than assumed to agree.
    fn inherited_arguments_compatible(
        &self,
        sup: &str,
        sup_args: &Option<Vec<crate::ast::Expression>>,
        sub: &str,
        sub_args: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> bool {
        let Some(sup_args) = sup_args else {
            return true;
        };
        let Some(inherited) = self.supertype_arguments(sub, sub_args.as_deref(), sup) else {
            return false;
        };
        sup_args.len() == inherited.len()
            && sup_args.iter().zip(&inherited).all(|(written, pinned)| {
                pinned
                    .as_ref()
                    .is_some_and(|pinned| self.argument_agrees_with(written, pinned, context))
            })
    }

    /// Whether the written type argument `written` names what the clauses pin
    /// its parameter to, `pinned` — a value for a value parameter.
    fn argument_agrees_with(
        &self,
        written: &crate::ast::Expression,
        pinned: &Type,
        context: &Context,
    ) -> bool {
        if let Some(value) = super::generics::extract_value_generic(pinned) {
            return Self::value_arguments_agree(written, value, context) == Some(true);
        }
        self.extract_type_from_expression(written)
            .is_ok_and(|written| self.type_arguments_agree(&written, pinned, context))
    }

    /// Whether two type arguments name the same type. An instance's
    /// argument is read and written through it alike, so neither side may
    /// stand for a subtype of the other: a `Box<Dog>` handed on as a
    /// `Box<Animal>` would be stored an `Animal` its `Dog` readers never see.
    pub(crate) fn type_arguments_agree(
        &self,
        first: &Type,
        second: &Type,
        context: &Context,
    ) -> bool {
        // A scalar argument fixes the width of every value the type holds at
        // it, so two agree only when they are the same scalar. The widening
        // and literal rules a single value is stored under would let an
        // `int` argument agree with every integer, and a `List<i32>` be read
        // as a `List<int>` at twice its element width.
        if crate::ast::types::scalar_width(&first.kind).is_some()
            && crate::ast::types::scalar_width(&second.kind).is_some()
        {
            return first.kind == second.kind;
        }
        self.are_compatible(first, second, context) && self.are_compatible(second, first, context)
    }

    /// Whether one of these spellings of `name` is a generic enum whose
    /// instantiation is still open, the other naming it.
    ///
    /// Only an enum: a class written without its arguments is reported where it
    /// is declared, and accepting one here would hide that.
    fn enum_instantiation_is_open(
        &self,
        name: &str,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> bool {
        if args1.is_some() == args2.is_some() {
            return false;
        }
        let definition = context
            .resolve_type_definition(name)
            .or_else(|| self.type_table.global_type_definitions.get(name));
        matches!(definition, Some(TypeDefinition::Enum(def)) if def.generics.is_some())
    }

    /// Checks if generic arguments are compatible.
    fn check_generic_args_compatible(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> bool {
        match (args1, args2) {
            (Some(a1), Some(a2)) => {
                if a1.len() != a2.len() {
                    return false;
                }
                for (arg1, arg2) in a1.iter().zip(a2.iter()) {
                    if let Some(agree) = Self::value_arguments_agree(arg1, arg2, context) {
                        if !agree {
                            return false;
                        }
                        continue;
                    }
                    let t1 = self
                        .extract_type_from_expression(arg1)
                        .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                    let t2 = self
                        .extract_type_from_expression(arg2)
                        .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                    if !self.type_arguments_agree(&t1, &t2, context) {
                        return false;
                    }
                }
                true
            }
            (None, None) => true,
            _ => false, // Mismatch in generic args presence
        }
    }

    /// Whether two generic arguments that stand for values denote the same
    /// value, or `None` when both are types and compare as types.
    ///
    /// A value argument is what a value-generic class is laid out from, so
    /// `Wrap<int, 3>` and `Wrap<int, 2>` are different types. Each side folds to
    /// the constant it denotes, a named `const` included. Two sides that do not
    /// fold agree only when they name the same value parameter; a value against
    /// a type, or a value that cannot be compared, is a mismatch.
    fn value_arguments_agree(
        arg1: &crate::ast::Expression,
        arg2: &crate::ast::Expression,
        context: &Context,
    ) -> Option<bool> {
        let (v1, v2) = (value_argument(arg1), value_argument(arg2));
        if v1.is_none() && v2.is_none() {
            return None;
        }
        let (Some(v1), Some(v2)) = (v1, v2) else {
            return Some(false);
        };
        let folded = (
            Self::try_eval_const_int_with_context(v1, context),
            Self::try_eval_const_int_with_context(v2, context),
        );
        Some(match folded {
            (Some(n1), Some(n2)) => n1 == n2,
            _ => matches!(
                (&v1.node, &v2.node),
                (ExpressionKind::Identifier(n1, None), ExpressionKind::Identifier(n2, None))
                    if n1 == n2
            ),
        })
    }

    /// Checks collection type compatibility (List, Set, Map, Array).
    ///
    /// After normalization, all built-in collection types are represented as
    /// `TypeKind::Custom("List"/"Array"/"Map"/"Set", args)`.  The canonical
    /// `TypeKind::List/Array/Map/Set` variants are never produced by downstream
    /// code and are guarded with `unreachable!`.
    fn check_collection_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        // Canonical variants are normalized away before this point.
        debug_assert!(
            !matches!(
                &t1.kind,
                TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_)
            ),
            "collection canonical variant reached check_collection_compatibility: {:?}",
            t1.kind
        );
        debug_assert!(
            !matches!(
                &t2.kind,
                TypeKind::List(_) | TypeKind::Array(_, _) | TypeKind::Map(_, _) | TypeKind::Set(_)
            ),
            "collection canonical variant reached check_collection_compatibility: {:?}",
            t2.kind
        );

        match (&t1.kind, &t2.kind) {
            (TypeKind::Custom(n1, args1), TypeKind::Custom(n2, args2)) if n1 == n2 => {
                self.check_builtin_collection(n1, args1, args2, context)
            }
            (TypeKind::Option(inner1), TypeKind::Option(inner2)) => {
                if matches!(inner2.kind, TypeKind::Void) {
                    return Some(true);
                }
                Some(self.are_compatible(inner1, inner2, context))
            }
            (TypeKind::Option(inner1), _) => Some(self.are_compatible(inner1, t2, context)),
            (TypeKind::Result(ok1, err1), TypeKind::Result(ok2, err2)) => {
                Some(self.check_result_compatible(ok1, err1, ok2, err2, context))
            }
            _ => None,
        }
    }

    /// Checks builtin collection (Array/List/Set/Map) compatibility.
    fn check_builtin_collection(
        &self,
        name: &str,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> Option<bool> {
        match BuiltinCollectionKind::from_name(name) {
            Some(BuiltinCollectionKind::Array) => {
                self.check_array_compatibility(args1, args2, context)
            }
            Some(BuiltinCollectionKind::List) | Some(BuiltinCollectionKind::Set) => {
                self.check_list_set_compatibility(args1, args2, context)
            }
            Some(BuiltinCollectionKind::Map) => self.check_map_compatibility(args1, args2, context),
            None => None,
        }
    }

    /// Checks array type compatibility (element type and size).
    fn check_array_compatibility(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> Option<bool> {
        let (inner1, size1) = match args1.as_deref() {
            Some([i, s, ..]) => (i, s),
            _ => return Some(false),
        };
        let (inner2, size2) = match args2.as_deref() {
            Some([i, s, ..]) => (i, s),
            _ => return Some(false),
        };
        if !self.check_inner_type_compatible(inner1, inner2, context) {
            return Some(false);
        }
        // Fold each size with the constant context so a named `const` in a
        // type-position `Array<T, SIZE>` (struct field, parameter, return)
        // compares equal to the literal the constructor form produces.
        match (
            Self::try_eval_const_int_with_context(size1, context),
            Self::try_eval_const_int_with_context(size2, context),
        ) {
            (Some(s1), Some(s2)) => Some(s1 == s2),
            _ => Some(size1 == size2),
        }
    }

    /// Checks List/Set type compatibility (element type only).
    fn check_list_set_compatibility(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> Option<bool> {
        let inner1 = match args1.as_deref() {
            Some([i, ..]) => i,
            _ => return Some(false),
        };
        let inner2 = match args2.as_deref() {
            Some([i, ..]) => i,
            _ => return Some(false),
        };
        Some(self.check_inner_type_compatible(inner1, inner2, context))
    }

    /// Checks Map type compatibility (key and value types).
    fn check_map_compatibility(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> Option<bool> {
        let (k1, v1) = match args1.as_deref() {
            Some([k, v, ..]) => (k, v),
            _ => return Some(false),
        };
        let (k2, v2) = match args2.as_deref() {
            Some([k, v, ..]) => (k, v),
            _ => return Some(false),
        };
        if let (Ok(k2_t), Ok(v2_t)) = (
            self.extract_type_from_expression(k2),
            self.extract_type_from_expression(v2),
        ) {
            // Empty map compatible with any map type
            if matches!(k2_t.kind, TypeKind::Void) && matches!(v2_t.kind, TypeKind::Void) {
                return Some(true);
            }
            if let (Ok(k1_t), Ok(v1_t)) = (
                self.extract_type_from_expression(k1),
                self.extract_type_from_expression(v1),
            ) {
                return Some(
                    self.type_arguments_agree(&k1_t, &k2_t, context)
                        && self.type_arguments_agree(&v1_t, &v2_t, context),
                );
            }
        }
        Some(false)
    }

    /// Whether `kind` is a floating-point type, `float` included.
    pub(crate) fn is_float_kind(kind: &TypeKind) -> bool {
        matches!(
            kind,
            TypeKind::Float | TypeKind::F16 | TypeKind::F32 | TypeKind::F64
        )
    }

    pub(crate) fn check_inner_type_compatible(
        &self,
        inner1: &crate::ast::Expression,
        inner2: &crate::ast::Expression,
        context: &Context,
    ) -> bool {
        if let Ok(t2_inner) = self.extract_type_from_expression(inner2) {
            // Empty collection compatible with any element type
            if matches!(t2_inner.kind, TypeKind::Void) {
                return true;
            }
            if let Ok(t1_inner) = self.extract_type_from_expression(inner1) {
                // Special case: Int literal vs specific integer type
                if matches!(t2_inner.kind, TypeKind::Int)
                    && self.is_integer(&t1_inner)
                    && !matches!(t1_inner.kind, TypeKind::Int)
                {
                    return false;
                }
                // Same rule for floats: collection storage is laid out at the
                // exact element width, so mixing `Float`/`F32`/`F64` across
                // a List/Array/Set/Map boundary produces silent layout
                // mismatch (a 4-byte literal stored into an 8-byte slot, or
                // vice versa, reads garbage on the other side). Scalar
                // widening still passes through `are_compatible`; this guard
                // only fires for collection inner types.
                if Self::is_float_kind(&t1_inner.kind)
                    && Self::is_float_kind(&t2_inner.kind)
                    && t1_inner.kind != t2_inner.kind
                {
                    return false;
                }
                // A collection is read and written through alike, so its
                // element type is invariant: a `List<Dog>` handed on as a
                // `List<Animal>` would be pushed an `Animal` its `Dog` readers
                // never see, and a `List<i8>` as a `List<i64>` written 8-byte
                // elements into 1-byte slots.
                return self.type_arguments_agree(&t1_inner, &t2_inner, context);
            }
        }
        false
    }

    /// Checks Result type compatibility (for legacy TypeKind::Result pairs).
    fn check_result_compatible(
        &self,
        ok1: &crate::ast::Expression,
        err1: &crate::ast::Expression,
        ok2: &crate::ast::Expression,
        err2: &crate::ast::Expression,
        context: &Context,
    ) -> bool {
        if let (Ok(ok1_t), Ok(err1_t), Ok(ok2_t), Ok(err2_t)) = (
            self.extract_type_from_expression(ok1),
            self.extract_type_from_expression(err1),
            self.extract_type_from_expression(ok2),
            self.extract_type_from_expression(err2),
        ) {
            let ok_compatible = matches!(ok1_t.kind, TypeKind::Void)
                || matches!(ok2_t.kind, TypeKind::Void)
                || self.are_compatible(&ok1_t, &ok2_t, context);
            let err_compatible = matches!(err1_t.kind, TypeKind::Void)
                || matches!(err2_t.kind, TypeKind::Void)
                || self.are_compatible(&err1_t, &err2_t, context);
            return ok_compatible && err_compatible;
        }
        false
    }

    /// Checks `Custom("Result", [ok, err])` arg compatibility.
    ///
    /// Void on either side of any arg is treated as a wildcard — `Ok(x)` infers
    /// `Result<T, void>` (unknown error side) and `Err(e)` infers `Result<void, E>`
    /// (unknown ok side). When such a partial result is assigned to a typed variable
    /// the void must be compatible with whatever the declared type says.
    fn check_result_args_compatible(
        &self,
        args1: &Option<Vec<crate::ast::Expression>>,
        args2: &Option<Vec<crate::ast::Expression>>,
        context: &Context,
    ) -> bool {
        let void_type = crate::ast::factory::make_type(TypeKind::Void);
        let get_arg = |args: &Option<Vec<crate::ast::Expression>>, idx: usize| {
            args.as_ref()
                .and_then(|v| v.get(idx))
                .and_then(|e| self.extract_type_from_expression(e).ok())
                .unwrap_or_else(|| void_type.clone())
        };

        let ok1 = get_arg(args1, 0);
        let err1 = get_arg(args1, 1);
        let ok2 = get_arg(args2, 0);
        let err2 = get_arg(args2, 1);

        let ok_compatible = matches!(ok1.kind, TypeKind::Void)
            || matches!(ok2.kind, TypeKind::Void)
            || self.are_compatible(&ok1, &ok2, context);
        let err_compatible = matches!(err1.kind, TypeKind::Void)
            || matches!(err2.kind, TypeKind::Void)
            || self.are_compatible(&err1, &err2, context);

        ok_compatible && err_compatible
    }

    /// Checks tuple type compatibility (element-wise).
    fn check_tuple_compatibility(&self, t1: &Type, t2: &Type, context: &Context) -> Option<bool> {
        if let (TypeKind::Tuple(e1), TypeKind::Tuple(e2)) = (&t1.kind, &t2.kind) {
            if e1.len() != e2.len() {
                return Some(false);
            }
            for (a_expr, b_expr) in e1.iter().zip(e2.iter()) {
                let a = self
                    .extract_type_from_expression(a_expr)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                let b = self
                    .extract_type_from_expression(b_expr)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                // Invariant, like every other type argument: a tuple is stored
                // element by element at its own layout, and nothing converts a
                // tuple value, so `(int, String)` read as `(int?, String)`
                // takes its bare word for a box. A tuple literal still builds
                // at a wider declared type, through `literal_fits`.
                if !self.type_arguments_agree(&a, &b, context) {
                    return Some(false);
                }
            }
            return Some(true);
        }
        None
    }

    /// Checks function type compatibility.
    fn check_function_compatibility(
        &self,
        t1: &Type,
        t2: &Type,
        context: &Context,
    ) -> Option<bool> {
        if let (TypeKind::Function(f1), TypeKind::Function(f2)) = (&t1.kind, &t2.kind) {
            // Check generics count
            let gen1_len = f1.generics.as_ref().map(|v| v.len()).unwrap_or(0);
            let gen2_len = f2.generics.as_ref().map(|v| v.len()).unwrap_or(0);
            if gen1_len != gen2_len {
                return Some(false);
            }

            // Check parameters
            if f1.params.len() != f2.params.len() {
                return Some(false);
            }

            // A function value is called through the signature it is stored
            // at, so each parameter is passed at the stored type's layout and
            // the result read back at it. Parameters and result are therefore
            // invariant: a `fn(d Dog)` stored as a `fn(a Animal)` would be
            // handed an `Animal`, and a `fn() int` stored as a `fn() int?`
            // would be read at an optional's layout it never wrote. A scalar
            // agrees only with the same scalar, as a type argument does: a
            // `fn(x i32) i32` called as a `fn(x int) int` would truncate
            // every argument and result to 32 bits.
            for (p1, p2) in f1.params.iter().zip(f2.params.iter()) {
                let t1 = self
                    .extract_type_from_expression(&p1.typ)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                let t2 = self
                    .extract_type_from_expression(&p2.typ)
                    .unwrap_or(crate::ast::factory::make_type(TypeKind::Error));
                if !self.type_arguments_agree(&t1, &t2, context) {
                    return Some(false);
                }
            }

            // Check return type
            let r1 = f1
                .return_type
                .as_ref()
                .and_then(|r| self.extract_type_from_expression(r).ok())
                .unwrap_or(crate::ast::factory::make_type(TypeKind::Void));
            let r2 = f2
                .return_type
                .as_ref()
                .and_then(|r| self.extract_type_from_expression(r).ok())
                .unwrap_or(crate::ast::factory::make_type(TypeKind::Void));

            return Some(self.type_arguments_agree(&r1, &r2, context));
        }
        None
    }

    /// Checks generic type compatibility.
    fn check_generic_compatibility(&self, t1: &Type, t2: &Type, context: &Context) -> Option<bool> {
        if let TypeKind::Generic(name1, constraint, kind) = &t1.kind {
            return Some(self.check_generic_in_t1(name1, constraint, kind, t2, context));
        }

        if let TypeKind::Generic(name2, constraint2, kind) = &t2.kind {
            return Some(self.check_generic_in_t2(name2, constraint2, kind, t1, context));
        }

        None
    }

    /// Checks whether a value of type `t2` may be stored where the generic
    /// `t1` is expected.
    ///
    /// A parameter a declaration in scope introduces accepts only itself, or a
    /// parameter whose `extends` bounds lead to it. A name nothing declares is
    /// an inference slot a call fills in, and accepts what meets its bound.
    fn check_generic_in_t1(
        &self,
        name1: &str,
        constraint: &Option<Box<Type>>,
        kind: &TypeDeclarationKind,
        t2: &Type,
        context: &Context,
    ) -> bool {
        if let TypeKind::Generic(name2, constraint2, kind2) = &t2.kind {
            if name1 == name2 || Self::bound_reaches(name1, constraint2, kind2) {
                return true;
            }
            if Self::is_declared_parameter(name1, context)
                && Self::is_declared_parameter(name2, context)
            {
                return false;
            }
            if constraint.is_none() && !Self::is_declared_parameter(name2, context) {
                return true;
            }
        } else if Self::is_declared_parameter(name1, context) {
            // Every caller binds the parameter to a type of its own choosing,
            // so no type the body can name is one — not even one that meets
            // the parameter's bound.
            return false;
        }
        if let Some(c) = constraint {
            return self.satisfies_constraint(t2, c, kind, context);
        }
        true
    }

    /// Whether a parameter bounded by `constraint` is, through its chain of
    /// `extends` bounds, the parameter `target`.
    ///
    /// `U extends T` is a `T`; `U extends Animal` is not a `T extends Animal`,
    /// since a caller binds `T` to a class `U` need not be. The chain is read
    /// off the bound types themselves, each resolved where its parameter was
    /// declared, never by looking a name up again: while a call is checked the
    /// callee's parameters are in scope too, and a caller's `U` looked up by
    /// name would find the callee's `U` and its bound.
    fn bound_reaches(
        target: &str,
        constraint: &Option<Box<Type>>,
        kind: &TypeDeclarationKind,
    ) -> bool {
        std::iter::successors(Self::extends_bound(kind, constraint), |bound| {
            let TypeKind::Generic(_, next, next_kind) = &bound.kind else {
                return None;
            };
            Self::extends_bound(next_kind, next)
        })
        .any(|bound| matches!(&bound.kind, TypeKind::Generic(name, _, _) if name == target))
    }

    /// The type an `extends` bound names, when the declaration has one.
    fn extends_bound<'t>(
        kind: &TypeDeclarationKind,
        constraint: &'t Option<Box<Type>>,
    ) -> Option<&'t Type> {
        match kind {
            TypeDeclarationKind::Extends => constraint.as_deref(),
            TypeDeclarationKind::None
            | TypeDeclarationKind::Is
            | TypeDeclarationKind::Implements
            | TypeDeclarationKind::Includes => None,
        }
    }

    /// The parameter a type written as a bare name stands for, when a
    /// declaration in scope introduces a type parameter of that name.
    ///
    /// A function or tuple type keeps its parts as written, so the `T` in a
    /// parameter written `fn(T) T` is read back as the name `T` while the same
    /// `T` reached through the parameter list is the parameter itself. Both are
    /// one type, and are compared as one.
    ///
    /// The name is resolved in the scope current at the comparison. That is
    /// the declaring scope everywhere but one place: while a call's arguments
    /// are checked, the callee's parameters are defined over the caller's, so
    /// a caller's `T` written inside a function type reads as the callee's `T`
    /// and carries the callee's bound.
    // TODO: resolve the names inside a written function or tuple type to
    // parameters once, where the signature is resolved, so this comparison
    // never looks a name up and a callee's parameter cannot shadow a caller's.
    fn written_parameter(ty: &Type, context: &Context) -> Option<Type> {
        let TypeKind::Custom(name, None) = &ty.kind else {
            return None;
        };
        let Some(TypeDefinition::Generic(def)) = context.resolve_type_definition(name) else {
            return None;
        };
        Some(Type::new(
            TypeKind::Generic(name.clone(), def.constraint.clone().map(Box::new), def.kind),
            ty.span,
        ))
    }

    /// Whether `name` is a type parameter a declaration in scope introduces,
    /// as opposed to an inference slot a call fills in.
    fn is_declared_parameter(name: &str, context: &Context) -> bool {
        matches!(
            context.resolve_type_definition(name),
            Some(TypeDefinition::Generic(_))
        )
    }

    /// Checks compatibility when t2 is a generic type.
    fn check_generic_in_t2(
        &self,
        name2: &str,
        constraint2: &Option<Box<Type>>,
        kind: &TypeDeclarationKind,
        t1: &Type,
        context: &Context,
    ) -> bool {
        if let Some(c) = constraint2 {
            if matches!(kind, TypeDeclarationKind::Extends) {
                return self.are_compatible(t1, c, context);
            }
            return false;
        }
        if !matches!(
            context.resolve_type_definition(name2),
            Some(TypeDefinition::Generic(_))
        ) {
            return true;
        }
        false
    }

    /// Checks if a type is a subtype of another (inheritance, interfaces, mixins).
    pub(crate) fn is_subtype(&self, sub: &str, sup: &str) -> bool {
        if sub == sup {
            return true;
        }

        if let Some(relation) = self.type_table.hierarchy.get(sub) {
            // Check extends
            if let Some(parent) = &relation.extends {
                if self.is_subtype(parent, sup) {
                    return true;
                }
            }
            // Check implements
            for interface in &relation.implements {
                if self.is_subtype(interface, sup) {
                    return true;
                }
            }
            // Check includes
            for mixin in &relation.includes {
                if self.is_subtype(mixin, sup) {
                    return true;
                }
            }
        }
        // A trait is every trait it extends. The class hierarchy does not
        // record that, so a class implementing `Titled` reaches `Named` through
        // `trait Titled extends Named` here.
        super::context::trait_is_or_extends(sub, sup, self.type_definitions())
    }

    /// Checks if a type satisfies a constraint.
    pub(crate) fn satisfies_constraint(
        &self,
        ty: &Type,
        constraint: &Type,
        kind: &TypeDeclarationKind,
        context: &Context,
    ) -> bool {
        match kind {
            TypeDeclarationKind::Extends => self.are_compatible(constraint, ty, context),
            TypeDeclarationKind::Implements => self.check_implements(ty, constraint, context),
            TypeDeclarationKind::Includes => self.check_includes(ty, constraint, context),
            TypeDeclarationKind::Is => ty == constraint,
            TypeDeclarationKind::None => true,
        }
    }

    /// Checks if a type implements an interface (structural typing for structs).
    pub(crate) fn check_implements(&self, ty: &Type, constraint: &Type, context: &Context) -> bool {
        let (constraint_name, ty_name) = match (&constraint.kind, &ty.kind) {
            (TypeKind::Custom(cn, _), TypeKind::Custom(tn, _)) => (cn.as_str(), tn.as_str()),
            _ => return false,
        };

        // Check hierarchy first
        if self.is_subtype(ty_name, constraint_name) {
            return true;
        }

        // Structural typing for structs
        let constraint_def = context
            .resolve_type_definition(constraint_name)
            .or_else(|| self.type_table.global_type_definitions.get(constraint_name));

        let ty_def = context
            .resolve_type_definition(ty_name)
            .or_else(|| self.type_table.global_type_definitions.get(ty_name));

        match (constraint_def, ty_def) {
            (Some(TypeDefinition::Struct(c_def)), Some(TypeDefinition::Struct(t_def))) => {
                // Check that ty has all fields of constraint
                for (c_name, c_type, _) in &c_def.fields {
                    if let Some((_, t_type, _)) =
                        t_def.fields.iter().find(|(t_name, _, _)| t_name == c_name)
                    {
                        if !self.are_compatible(c_type, t_type, context) {
                            return false;
                        }
                    } else {
                        return false;
                    }
                }
                true
            }
            _ => false,
        }
    }

    /// Checks if a type includes another (mixin pattern).
    pub(crate) fn check_includes(&self, ty: &Type, constraint: &Type, context: &Context) -> bool {
        let (constraint_name, ty_name) = match (&constraint.kind, &ty.kind) {
            (TypeKind::Custom(cn, _), TypeKind::Custom(tn, _)) => (cn.as_str(), tn.as_str()),
            _ => return false,
        };

        // Check hierarchy first
        if self.is_subtype(ty_name, constraint_name) {
            return true;
        }

        // Structural checking
        let constraint_def = context
            .resolve_type_definition(constraint_name)
            .or_else(|| self.type_table.global_type_definitions.get(constraint_name));

        let ty_def = context
            .resolve_type_definition(ty_name)
            .or_else(|| self.type_table.global_type_definitions.get(ty_name));

        match constraint_def {
            Some(TypeDefinition::Class(class_def)) => {
                self.check_class_methods_included(class_def, ty_def, context)
            }
            Some(TypeDefinition::Trait(trait_def)) => {
                self.check_trait_methods_included(trait_def, ty_def, context)
            }
            Some(TypeDefinition::Struct(struct_def)) => {
                self.check_struct_fields_included(struct_def, ty_def, context)
            }
            _ => false,
        }
    }

    /// Checks that a type includes all methods from a class.
    fn check_class_methods_included(
        &self,
        class_def: &super::context::ClassDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_methods = match ty_def {
            Some(TypeDefinition::Class(td)) => &td.methods,
            _ => return false,
        };

        for (method_name, method_info) in &class_def.methods {
            if let Some(ty_method) = ty_methods.get(method_name) {
                if !self.check_method_compatible(ty_method, method_info, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that a type includes all methods from a trait.
    fn check_trait_methods_included(
        &self,
        trait_def: &super::context::TraitDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_methods = match ty_def {
            Some(TypeDefinition::Class(td)) => &td.methods,
            _ => return false,
        };

        for (method_name, method_info) in &trait_def.methods {
            if let Some(ty_method) = ty_methods.get(method_name) {
                if !self.check_method_compatible(ty_method, method_info, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that a type includes all fields from a struct.
    fn check_struct_fields_included(
        &self,
        struct_def: &super::context::StructDefinition,
        ty_def: Option<&TypeDefinition>,
        context: &Context,
    ) -> bool {
        let ty_fields = match ty_def {
            Some(TypeDefinition::Struct(td)) => &td.fields,
            _ => return false,
        };

        for (c_name, c_type, _) in &struct_def.fields {
            if let Some((_, t_type, _)) = ty_fields.iter().find(|(t_name, _, _)| t_name == c_name) {
                if !self.are_compatible(c_type, t_type, context) {
                    return false;
                }
            } else {
                return false;
            }
        }
        true
    }

    /// Checks that two method signatures are compatible.
    fn check_method_compatible(
        &self,
        ty_method: &super::context::MethodInfo,
        expected_method: &super::context::MethodInfo,
        context: &Context,
    ) -> bool {
        // Check parameter count
        if ty_method.params.len() != expected_method.params.len() {
            return false;
        }

        // Check parameter types
        for ((_, p_type), (_, c_type)) in ty_method.params.iter().zip(expected_method.params.iter())
        {
            if !self.are_compatible(p_type, c_type, context) {
                return false;
            }
        }

        // Check return type
        self.are_compatible(
            &ty_method.return_type,
            &expected_method.return_type,
            context,
        )
    }

    /// Returns `true` if the type represents a string, either the built-in
    /// `TypeKind::String` or the class `TypeKind::Custom(STRING_TYPE_NAME, _)`.
    pub(crate) fn is_string_type(&self, ty: &Type) -> bool {
        match &ty.kind {
            TypeKind::String => true,
            TypeKind::Custom(name, _) => name == STRING_TYPE_NAME,
            _ => false,
        }
    }
}

/// The value a generic argument stands for: the expression a value-generic
/// marker wraps, or the argument itself when it is written as a value rather
/// than a type. `None` for a type argument.
fn value_argument(arg: &crate::ast::Expression) -> Option<&crate::ast::Expression> {
    match &arg.node {
        ExpressionKind::Type(ty, _) => super::generics::extract_value_generic(ty),
        _ => Some(arg),
    }
}
