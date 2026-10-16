// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

pub mod constant_propagation;
pub mod copy_propagation;
pub mod dead_code;
pub mod owned_params;
pub mod perceus;
pub mod rc_elision;
pub mod simplify_cfg;
pub mod unshare_elision;
