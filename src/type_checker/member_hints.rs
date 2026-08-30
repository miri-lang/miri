// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Synonym-based suggestions for member access errors.
//!
//! When a member (field or method) is not found, this module suggests
//! plausible alternatives based on:
//! - Foreign-language synonym groups (e.g., `len`, `length`, `size`, `count`)
//! - Arity preference (prefer same-arity method when at a call site)
//! - Standard English-language naming idiom matching
//!
//! Suggestions are only emitted for names actually present on the receiver's
//! member list, never for stdlib-hardcoded type names. The compiler treats the
//! standard library as user code and never assumes what its API names are.

use crate::error::format::find_best_match;

/// A member candidate (field or method) with its name and arity if applicable.
#[derive(Debug, Clone)]
pub(crate) struct MemberCandidate<'a> {
    pub name: &'a str,
    pub arity: Option<usize>,
}

impl<'a> MemberCandidate<'a> {
    pub fn field(name: &'a str) -> Self {
        Self { name, arity: None }
    }

    pub fn method(name: &'a str, param_count: usize) -> Self {
        Self {
            name,
            arity: Some(param_count),
        }
    }
}

/// A group of method names that are synonymous across programming languages.
struct SynonymGroup {
    names: &'static [&'static str],
}

impl SynonymGroup {
    fn contains(&self, name: &str) -> bool {
        self.names.contains(&name)
    }

    fn members_in_candidates<'a>(
        &self,
        candidates: &[MemberCandidate<'a>],
    ) -> Vec<MemberCandidate<'a>> {
        candidates
            .iter()
            .filter(|c| self.contains(c.name))
            .cloned()
            .collect()
    }
}

/// Length-related method names across programming languages.
const LEN_GROUP: SynonymGroup = SynonymGroup {
    names: &["len", "length", "size", "count"],
};

/// Append/add-to-collection method names.
const APPEND_GROUP: SynonymGroup = SynonymGroup {
    names: &["append", "add", "push", "push_back", "add_last"],
};

/// String uppercase method names.
const UPPER_GROUP: SynonymGroup = SynonymGroup {
    names: &["upper", "uppercase", "to_upper", "to_uppercase", "upcase"],
};

/// String lowercase method names.
const LOWER_GROUP: SynonymGroup = SynonymGroup {
    names: &["lower", "lowercase", "to_lower", "to_lowercase", "downcase"],
};

/// Collection accessor/iterator method names.
const ITERATOR_GROUP: SynonymGroup = SynonymGroup {
    names: &["keys", "values", "items", "entries"],
};

/// Empty-check method names.
const EMPTY_GROUP: SynonymGroup = SynonymGroup {
    names: &["is_empty", "empty", "is_blank"],
};

/// Index search method names.
const INDEX_OF_GROUP: SynonymGroup = SynonymGroup {
    names: &["index_of", "find_index", "position"],
};

/// Membership check method names.
const CONTAINS_GROUP: SynonymGroup = SynonymGroup {
    names: &["contains", "includes", "has"],
};

/// All synonym groups in priority order.
const SYNONYM_GROUPS: &[&SynonymGroup] = &[
    &LEN_GROUP,
    &APPEND_GROUP,
    &UPPER_GROUP,
    &LOWER_GROUP,
    &ITERATOR_GROUP,
    &EMPTY_GROUP,
    &INDEX_OF_GROUP,
    &CONTAINS_GROUP,
];

/// Suggests a member name based on foreign-language synonyms and arity preference.
///
/// Returns `Some(suggestion)` if:
/// 1. The failing name belongs to a synonym group with members present in candidates
/// 2. Or, a same-arity candidate exists within edit distance threshold
///
/// Otherwise returns `None`.
pub(crate) fn suggest_member(
    failing_name: &str,
    candidates: &[MemberCandidate],
    call_site_arity: Option<usize>,
) -> Option<String> {
    // Check each synonym group
    for group in SYNONYM_GROUPS {
        if group.contains(failing_name) {
            // Find members in this group that are present in candidates
            let group_members = group.members_in_candidates(candidates);
            if !group_members.is_empty() {
                // Pick the closest match by Levenshtein distance
                if let Some(best) = group_members.iter().min_by_key(|c| {
                    crate::error::format::levenshtein_distance(failing_name, c.name)
                }) {
                    return Some(best.name.to_string());
                }
            }
        }
    }

    // If not a synonym group member, fall back to arity-preferring find_best_match
    arity_aware_find_best_match(failing_name, candidates, call_site_arity)
}

/// Like `find_best_match`, but prefers candidates matching the call site arity.
///
/// If `call_site_arity` is `Some(n)`, candidates with `arity == Some(n)` are
/// preferred (ranked first). Candidates outside the arity are only considered
/// if no same-arity candidate is within threshold.
fn arity_aware_find_best_match(
    target: &str,
    candidates: &[MemberCandidate],
    call_site_arity: Option<usize>,
) -> Option<String> {
    if let Some(arity) = call_site_arity {
        // First, try candidates matching this arity
        let same_arity_candidates: Vec<&str> = candidates
            .iter()
            .filter_map(|c| {
                if c.arity == Some(arity) {
                    Some(c.name)
                } else {
                    None
                }
            })
            .collect();

        if !same_arity_candidates.is_empty() {
            if let Some(match_str) = find_best_match(target, &same_arity_candidates) {
                return Some(match_str);
            }
        }
    }

    // Fall back to all candidates
    let all_names: Vec<&str> = candidates.iter().map(|c| c.name).collect();
    find_best_match(target, &all_names)
}

/// Suggests an iteration-style help message for iterator-accessor names.
///
/// Returns a help string if:
/// - The failing name is an iterator group member (keys/values/items/entries)
/// - No group member is present on the type
/// - The receiver type is iterable
///
/// Otherwise returns `None`.
pub(crate) fn suggest_iteration_help(
    failing_name: &str,
    candidates: &[MemberCandidate],
    type_name: &str,
    is_iterable: bool,
) -> Option<String> {
    // Only applies to iterator group members
    if !ITERATOR_GROUP.contains(failing_name) {
        return None;
    }

    // Only if no group member exists
    let has_member = ITERATOR_GROUP.members_in_candidates(candidates);
    if !has_member.is_empty() {
        return None;
    }

    // Only if the type is iterable
    if !is_iterable {
        return None;
    }

    Some(format!(
        "'{type_name}' is iterable: use a 'for' loop over it instead of '{failing_name}'."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_len_suggests_length() {
        let candidates = vec![
            MemberCandidate::field("length"),
            MemberCandidate::field("first"),
        ];
        let suggestion = suggest_member("len", &candidates, None);
        assert_eq!(suggestion, Some("length".to_string()));
    }

    #[test]
    fn test_len_suggests_size_when_length_absent() {
        let candidates = vec![
            MemberCandidate::field("size"),
            MemberCandidate::field("first"),
        ];
        let suggestion = suggest_member("len", &candidates, None);
        assert_eq!(suggestion, Some("size".to_string()));
    }

    #[test]
    fn test_len_prefers_length_over_size_same_distance() {
        // Both "length" and "size" have distance 3 from "len",
        // but "length" should be picked first due to BTreeMap ordering from candidates
        let candidates = vec![
            MemberCandidate::field("length"),
            MemberCandidate::field("size"),
        ];
        let suggestion = suggest_member("len", &candidates, None);
        assert_eq!(suggestion, Some("length".to_string()));
    }

    #[test]
    fn test_no_suggestion_when_no_synonyms() {
        let candidates = vec![MemberCandidate::field("foo"), MemberCandidate::field("bar")];
        let suggestion = suggest_member("len", &candidates, None);
        assert_eq!(suggestion, None);
    }

    #[test]
    fn test_arity_preference_zero_arg() {
        let candidates = vec![
            MemberCandidate::method("siz", 0),
            MemberCandidate::method("size", 1),
        ];
        let suggestion = suggest_member("len", &candidates, Some(0));
        assert_eq!(suggestion, Some("siz".to_string()));
    }

    #[test]
    fn test_arity_preference_one_arg() {
        let candidates = vec![
            MemberCandidate::method("siz", 0),
            MemberCandidate::method("size", 1),
        ];
        let suggestion = suggest_member("len", &candidates, Some(1));
        assert_eq!(suggestion, Some("size".to_string()));
    }

    #[test]
    fn test_iteration_help_on_iterable() {
        let candidates = vec![MemberCandidate::field("first")];
        let help = suggest_iteration_help("keys", &candidates, "MyList", true);
        assert!(help.is_some_and(|h| h.contains("for")));
    }

    #[test]
    fn test_no_iteration_help_on_non_iterable() {
        let candidates = vec![MemberCandidate::field("first")];
        let help = suggest_iteration_help("keys", &candidates, "MyObj", false);
        assert_eq!(help, None);
    }

    #[test]
    fn test_no_iteration_help_when_keys_exists() {
        let candidates = vec![
            MemberCandidate::field("keys"),
            MemberCandidate::field("first"),
        ];
        let help = suggest_iteration_help("keys", &candidates, "MyDict", true);
        assert_eq!(help, None);
    }
}
