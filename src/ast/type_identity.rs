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
/// is `module`, written as that path is: `local.k.a`.
pub fn qualified(module: &str, name: &str) -> String {
    let mut identity = String::with_capacity(module.len() + SEPARATOR.len() + name.len());
    identity.push_str(module);
    identity.push_str(SEPARATOR);
    identity.push_str(name);
    identity
}

/// Whether the qualified `identity` names a type the module whose `use` path
/// is `module` declares.
pub fn is_declared_in(identity: &str, module: &str) -> bool {
    identity
        .rsplit_once(SEPARATOR)
        .is_some_and(|(declaring, _)| declaring == module)
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

    #[test]
    fn a_qualified_identity_reads_back_as_its_module_and_name() {
        let identity = qualified("local.k.a", "Helper");
        assert_eq!(identity, "local.k.a.Helper");
        assert!(is_qualified(&identity));
        assert_eq!(source_name(&identity), "Helper");
        assert_eq!(
            module_path(&identity).collect::<Vec<_>>(),
            ["local", "k", "a"]
        );
        assert!(is_declared_in(&identity, "local.k.a"));
        assert!(!is_declared_in(&identity, "local.k"));
        assert!(!is_declared_in("Helper", "local.k.a"));
    }

    #[test]
    fn a_written_name_is_its_own_identity() {
        assert!(!is_qualified("Helper"));
        assert_eq!(source_name("Helper"), "Helper");
        assert_eq!(module_path("Helper").count(), 0);
    }
}
