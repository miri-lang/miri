// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What compiled code and the runtime agree on: how memory is laid out, and
//! what each bit of the GPU fault word means. Each file here is compiled into
//! the runtime as well, so it depends on nothing but `core`.

pub mod gpu_fault;
pub mod layout;
