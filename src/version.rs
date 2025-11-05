// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
