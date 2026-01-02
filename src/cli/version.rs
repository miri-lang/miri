// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

pub fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}
