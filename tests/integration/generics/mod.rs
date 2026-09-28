// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub use crate::integration::utils;

pub mod arithmetic;
pub mod bare_parameter_elements;
pub mod basic;
pub mod bounded_parameter_methods;
pub mod classes_inside_generic_functions;
pub mod declaration_order;
pub mod delegation;
pub mod distinct_parameters;
pub mod explicit_type_arguments;
pub mod extends;
pub mod generic_type_drop_thunks;
pub mod invariance;
pub mod lambdas;
pub mod operator_dispatch;
pub mod requirements_through_aliases;
pub mod return_type_parameters;
pub mod shared_body_releases;
pub mod structs;
pub mod trait_bounded_parameters;
pub mod unary_and_cast;
pub mod unregistered_drop_instantiations;
pub mod used_methods;
pub mod used_methods_shapes;
pub mod written_local_types;
