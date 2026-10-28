// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub use crate::integration::utils;

pub mod basic;
pub mod coalescing;
pub mod declared_optional_literals;
pub mod edge_cases;
pub mod held_bytes_round_trips;
pub mod inline_seams;
pub mod inline_values;
pub mod matching;
pub mod nested;
pub mod nullable_pointer;
pub mod nullable_pointer_aliases;
pub mod nullable_pointer_dispatch;
pub mod nullable_pointer_flows;
pub mod optional_arguments;
pub mod optional_elements;
pub mod optional_fields;
pub mod parameter_named_types;
pub mod rc_managed;
pub mod widened_payloads;
