// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `KNOWN_RED.toml`: the cells that are red today, grouped by the root-cause
//! family whose fix turns them green.
//!
//! The file is a gate in both directions. A red cell missing from it fails
//! the build, so a new defect in a known family reopens that family instead
//! of arriving as a fresh one-off; and a listed cell that has gone green
//! fails the build too, so a family is finished exactly when its section is
//! empty.

use std::collections::BTreeMap;
use std::path::PathBuf;

/// The families a red cell may be charged to, each fixed at one authority.
pub const FAMILIES: &[(&str, &str)] = &[
    (
        "A",
        "element layout: width, stride and by-address transport",
    ),
    ("B", "Set element and Map key identity: equals and hash"),
    ("C", "generic instantiation read after substitution"),
    ("D", "drop hooks, release paths and ownership transfer"),
    ("E", "operator-to-trait dispatch"),
    (
        "X",
        "outside the five families: each cell is its own defect",
    ),
];

pub fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/conformance/matrix/KNOWN_RED.toml")
}

/// Every listed cell, mapped to its family.
pub fn load() -> Result<BTreeMap<String, String>, String> {
    let text = std::fs::read_to_string(path()).map_err(|e| format!("read KNOWN_RED.toml: {e}"))?;
    let table: toml::Table = text
        .parse()
        .map_err(|e| format!("parse KNOWN_RED.toml: {e}"))?;
    let mut cells = BTreeMap::new();
    for (family, section) in &table {
        if !FAMILIES.iter().any(|(name, _)| name == family) {
            return Err(format!("KNOWN_RED.toml: unknown family `{family}`"));
        }
        let listed = section
            .get("cells")
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("KNOWN_RED.toml: [{family}] has no `cells` array"))?;
        for cell in listed {
            let name = cell
                .as_str()
                .ok_or_else(|| format!("KNOWN_RED.toml: [{family}] lists a non-string"))?;
            if let Some(previous) = cells.insert(name.to_string(), family.clone()) {
                return Err(format!(
                    "KNOWN_RED.toml: `{name}` is listed under both {previous} and {family}"
                ));
            }
        }
    }
    Ok(cells)
}
