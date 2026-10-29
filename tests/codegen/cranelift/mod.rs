// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod backend;
pub mod closure;
pub mod closure_trampolines;
pub mod collection_header;
pub mod dealloc;
pub mod determinism;
pub mod drop_thunk_boundary;
pub mod frame_pointers;
pub mod function_alignment;
pub mod inliner;
pub mod layout;
pub mod loop_invariants;
pub mod optional_layout;
pub mod predicates;
pub mod rc;
pub mod slot_promotion;
pub mod structural_elements;
pub mod translate_rvalue;
pub mod translate_statement;
pub mod translator;
pub mod types;
pub mod value_abi;
pub mod value_arguments;
pub mod value_locals;
pub mod wide_scalar_abi;
