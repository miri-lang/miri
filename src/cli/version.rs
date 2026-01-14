// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::sync::OnceLock;

/// Get version string for display.
pub fn version_string() -> String {
    let version = env!("CARGO_PKG_VERSION");
    let os = std::env::consts::OS;
    let arch = std::env::consts::ARCH;
    format!("{} ({}/{})", version, os, arch)
}

/// Get version string as a static reference (helper for clap).
pub fn version_ref() -> &'static str {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(version_string).as_str()
}
