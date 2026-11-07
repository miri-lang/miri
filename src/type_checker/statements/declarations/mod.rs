// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod class_def;
pub mod drop_hook;
pub mod enum_def;
pub mod func;
pub mod generic_method_rules;
pub mod generics;
pub mod inherited_traits;
pub mod method_names;
pub mod struct_def;
pub mod trait_def;

pub(crate) use class_def::ClassBodies;
pub(crate) use enum_def::{EnumBodies, EnumDeclaration};
pub(crate) use func::FunctionDeclarationInfo;
