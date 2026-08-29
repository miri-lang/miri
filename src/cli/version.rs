// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::sync::OnceLock;

/// The compiler's crate version on its own.
///
/// [`version_string`] adds the platform it was built for, which belongs in a
/// bug report rather than in a file that reads the same everywhere.
pub fn crate_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

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
