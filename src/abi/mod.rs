// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What compiled code and the runtime agree on about memory. `layout.rs` is
//! compiled into the runtime as well, so it depends on nothing but `core`.

pub mod layout;
