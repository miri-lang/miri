// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The identity a declared type is known by once it is registered.
//!
//! A type is registered under the name its declaration writes, with one
//! exception: a type an imported module keeps private is registered under its
//! module path followed by that name — `local.k.a.Helper` — so that two
//! modules may each keep a private type of one name, and a program may declare
//! a type of the name a module keeps private. Every later stage keys by the
//! identity and never by the written name.
//!
//! A name a declaration writes is an identifier and never holds a `.`, so a
//! qualified identity is never a written name, and the last `.` of one
//! separates its module path from the name its declaration writes. This module
//! is the only place that spells an identity or reads one apart.

/// Separates the identifiers of a qualified identity.
const SEPARATOR: &str = ".";

/// The identity of the type declared as `name` in the module whose `use` path
/// is `module`.
pub fn qualified(module: &[String], name: &str) -> String {
    let mut identity = module.join(SEPARATOR);
    identity.push_str(SEPARATOR);
    identity.push_str(name);
    identity
}

/// The name the declaration of the type `identity` writes.
pub fn source_name(identity: &str) -> &str {
    identity
        .rsplit_once(SEPARATOR)
        .map_or(identity, |(_, name)| name)
}

/// The identifiers of the module path a qualified `identity` carries, in
/// order; none for an identity that is its written name.
pub fn module_path(identity: &str) -> impl Iterator<Item = &str> {
    identity
        .rsplit_once(SEPARATOR)
        .map(|(module, _)| module.split(SEPARATOR))
        .into_iter()
        .flatten()
}

/// Whether `identity` carries the module path of the module declaring it.
pub fn is_qualified(identity: &str) -> bool {
    identity.contains(SEPARATOR)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn path(segments: &[&str]) -> Vec<String> {
        segments.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn a_qualified_identity_reads_back_as_its_module_and_name() {
        let identity = qualified(&path(&["local", "k", "a"]), "Helper");
        assert_eq!(identity, "local.k.a.Helper");
        assert!(is_qualified(&identity));
        assert_eq!(source_name(&identity), "Helper");
        assert_eq!(
            module_path(&identity).collect::<Vec<_>>(),
            ["local", "k", "a"]
        );
    }

    #[test]
    fn a_written_name_is_its_own_identity() {
        assert!(!is_qualified("Helper"));
        assert_eq!(source_name("Helper"), "Helper");
        assert_eq!(module_path("Helper").count(), 0);
    }
}
