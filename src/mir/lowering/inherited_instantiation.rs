// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Lowering's path to the rules for what a class inherits at an
//! instantiation: which ancestor compiles an inherited method's body, the
//! arguments a parent is reached at through an `extends` clause, and the type
//! each inherited field is stored at. The rules themselves live in
//! [`crate::mir::instantiation::inherited`], shared with code generation.

pub(crate) use crate::mir::instantiation::inherited::{
    base_class_instantiation, declared_field_types, declaring_class_instantiation,
    own_parameters_left_open,
};
