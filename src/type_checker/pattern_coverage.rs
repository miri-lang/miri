// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Whether a set of match patterns covers every value they could be given.
//!
//! The patterns are a matrix: one row per pattern an arm can be taken by, one
//! column per value still to be matched. A column is decided by the
//! constructors its patterns name. When they name every constructor of their
//! type, each constructor is checked in turn against the rows it reaches, its
//! payloads spliced in as new columns. Otherwise the uncovered constructors
//! are left to the rows that match anything there. This is the usefulness
//! check of Maranget's "Warnings for pattern matching", asked of a row of
//! wildcards.
//!
//! The constructor set is read off the patterns themselves, so no type is
//! needed: a column holding `Shape.Circle(r)` is a column of `Shape`s.

use crate::ast::literal::Literal;
use crate::ast::pattern::Pattern;
use crate::ast::types::OPTION_TYPE_NAME;

/// One way a match can be entered: a pattern per column, `None` where any
/// value matches.
pub(crate) type CoverageRow<'p> = Vec<Option<&'p Pattern>>;

/// A value's outermost shape, as a pattern names it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Constructor {
    Some,
    None,
    Variant {
        enum_name: String,
        variant: String,
    },
    Bool(bool),
    Tuple(usize),
    /// A literal of a type whose values cannot all be listed: an integer,
    /// float or string literal, or a regex.
    Unlisted,
}

/// The variants an enum declares, each with how many payloads it carries;
/// `None` when the variant set is not known to be complete here.
pub(crate) type VariantLookup<'a> = dyn Fn(&str) -> Option<Vec<(String, usize)>> + 'a;

/// Answers coverage questions, looking enum variant sets up through `variants`.
pub(crate) struct Coverage<'a> {
    pub variants: &'a VariantLookup<'a>,
}

impl Coverage<'_> {
    /// Whether every value of the first column built by `constructor`, which
    /// carries `arity` payloads, is matched by some row.
    pub(crate) fn covers(
        &self,
        rows: &[CoverageRow],
        constructor: &Constructor,
        arity: usize,
    ) -> bool {
        self.is_exhaustive(&specialize(rows, constructor, arity))
    }

    /// Whether every combination of values for the columns is matched by
    /// some row.
    fn is_exhaustive(&self, rows: &[CoverageRow]) -> bool {
        let Some(first) = rows.first() else {
            return false;
        };
        if first.is_empty() {
            return true;
        }
        let heads: Vec<Constructor> = rows
            .iter()
            .filter_map(|row| row.first().copied().flatten().map(|p| constructor_of(p).0))
            .collect();
        match self.complete_signature(&heads) {
            Some(signature) => signature
                .iter()
                .all(|(constructor, arity)| self.covers(rows, constructor, *arity)),
            None => self.is_exhaustive(&default_rows(rows)),
        }
    }

    /// Every constructor of the type `heads` belong to, with its arity, when
    /// `heads` names all of them; `None` when one is missing or the type's
    /// values cannot be listed.
    fn complete_signature(&self, heads: &[Constructor]) -> Option<Vec<(Constructor, usize)>> {
        let signature = match heads.first()? {
            Constructor::Some | Constructor::None => {
                vec![(Constructor::Some, 1), (Constructor::None, 0)]
            }
            Constructor::Bool(_) => {
                vec![(Constructor::Bool(true), 0), (Constructor::Bool(false), 0)]
            }
            Constructor::Tuple(width) => vec![(Constructor::Tuple(*width), *width)],
            Constructor::Variant { enum_name, .. } => (self.variants)(enum_name)?
                .into_iter()
                .map(|(variant, arity)| {
                    let enum_name = enum_name.clone();
                    (Constructor::Variant { enum_name, variant }, arity)
                })
                .collect(),
            Constructor::Unlisted => return None,
        };
        signature
            .iter()
            .all(|(constructor, _)| heads.contains(constructor))
            .then_some(signature)
    }
}

/// The row a match arm's `pattern` contributes, matched against one value.
pub(crate) fn row_of(pattern: &Pattern) -> CoverageRow<'_> {
    vec![cell(Some(pattern))]
}

/// Whether `pattern` matches any value, binding it or discarding it.
fn is_wildcard(pattern: &Pattern) -> bool {
    matches!(pattern, Pattern::Identifier(_) | Pattern::Default)
}

/// A matrix cell for `pattern`: `None` when it matches any value.
fn cell(pattern: Option<&Pattern>) -> Option<&Pattern> {
    pattern.filter(|pattern| !is_wildcard(pattern))
}

/// The rows a value built by `constructor` can still take, with the
/// constructor's `arity` payloads in place of the first column. A row naming
/// another constructor drops out; a row matching anything there matches
/// anything in each payload.
fn specialize<'p>(
    rows: &[CoverageRow<'p>],
    constructor: &Constructor,
    arity: usize,
) -> Vec<CoverageRow<'p>> {
    rows.iter()
        .filter_map(|row| {
            let (head, rest) = row.split_first()?;
            let payloads: CoverageRow<'p> = match head {
                None => vec![None; arity],
                Some(pattern) => {
                    let (head_constructor, payloads) = constructor_of(pattern);
                    if head_constructor != *constructor {
                        return None;
                    }
                    (0..arity).map(|index| cell(payloads.get(index))).collect()
                }
            };
            Some(payloads.into_iter().chain(rest.iter().copied()).collect())
        })
        .collect()
}

/// The rows whose first column matches anything, without that column.
fn default_rows<'p>(rows: &[CoverageRow<'p>]) -> Vec<CoverageRow<'p>> {
    rows.iter()
        .filter_map(|row| match row.split_first()? {
            (None, rest) => Some(rest.to_vec()),
            (Some(_), _) => None,
        })
        .collect()
}

/// The constructor a non-wildcard `pattern` names, and the patterns written
/// for its payloads.
fn constructor_of(pattern: &Pattern) -> (Constructor, &[Pattern]) {
    match pattern {
        Pattern::Literal(Literal::None) => (Constructor::None, &[]),
        Pattern::Literal(Literal::Boolean(value)) => (Constructor::Bool(*value), &[]),
        Pattern::Member(type_pattern, variant) => (named_constructor(type_pattern, variant), &[]),
        Pattern::EnumVariant(parent, payloads) => {
            let constructor = match parent.as_ref() {
                Pattern::Identifier(name) if name == "Some" => Constructor::Some,
                Pattern::Member(type_pattern, variant) => named_constructor(type_pattern, variant),
                Pattern::Identifier(_)
                | Pattern::Literal(_)
                | Pattern::EnumVariant(..)
                | Pattern::Tuple(_)
                | Pattern::Regex(_)
                | Pattern::Default => Constructor::Unlisted,
            };
            (constructor, payloads)
        }
        Pattern::Tuple(elements) => (Constructor::Tuple(elements.len()), elements),
        Pattern::Literal(_) | Pattern::Regex(_) | Pattern::Identifier(_) | Pattern::Default => {
            (Constructor::Unlisted, &[])
        }
    }
}

/// The constructor `Type.variant` names.
fn named_constructor(type_pattern: &Pattern, variant: &str) -> Constructor {
    let Pattern::Identifier(type_name) = type_pattern else {
        return Constructor::Unlisted;
    };
    match (type_name.as_str(), variant) {
        (OPTION_TYPE_NAME, "Some") => Constructor::Some,
        (OPTION_TYPE_NAME, "None") => Constructor::None,
        _ => Constructor::Variant {
            enum_name: type_name.clone(),
            variant: variant.to_string(),
        },
    }
}
