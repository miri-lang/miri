// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The declarations a literal is built through, found by their attributes.
//!
//! A regex literal (`re"pattern"`) is a value of whatever class declares a
//! static method marked `@regex_literal`, built by calling that method on the
//! validated pattern; a regex match pattern tests its subject with the method
//! of the same class marked `@regex_match`. The compiler learns both from the
//! declarations and names no class or method of the standard library.

use std::collections::HashMap;

use crate::ast::{PATTERN_LITERAL_ATTRIBUTE, PATTERN_MATCH_ATTRIBUTE};
use crate::type_checker::context::{MethodInfo, TypeDefinition};

/// The class a regex literal is an instance of, and the methods it is built
/// and tested through.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegexLiteral {
    /// The class every regex literal is typed as.
    pub class: String,
    /// The static method marked `@regex_literal`.
    pub constructor: String,
    /// The method marked `@regex_match`, when the class declares one.
    pub matcher: Option<String>,
}

/// The regex literal hook among `definitions`: the class, first by name,
/// declaring a static method marked `@regex_literal`.
pub fn regex_literal(definitions: &HashMap<String, TypeDefinition>) -> Option<RegexLiteral> {
    let mut hooks: Vec<RegexLiteral> = (definitions.iter())
        .filter_map(|(name, definition)| {
            let TypeDefinition::Class(class) = definition else {
                return None;
            };
            let constructor = marked(&class.methods, PATTERN_LITERAL_ATTRIBUTE)
                .filter(|(_, method)| method.is_static)?;
            Some(RegexLiteral {
                class: name.clone(),
                constructor: constructor.0.clone(),
                matcher: marked(&class.methods, PATTERN_MATCH_ATTRIBUTE)
                    .map(|(name, _)| name.clone()),
            })
        })
        .collect();
    hooks.sort_by(|left, right| left.class.cmp(&right.class));
    hooks.into_iter().next()
}

/// The first method of `methods`, by name, carrying `attribute`.
fn marked<'m>(
    methods: &'m std::collections::BTreeMap<String, MethodInfo>,
    attribute: &str,
) -> Option<(&'m String, &'m MethodInfo)> {
    (methods.iter()).find(|(_, method)| method.attributes.iter().any(|attr| attr.name == attribute))
}
