// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Escape summary data structures for Phase 12 escape-based use-after-move
//! inference on managed types.
//!
//! A function's escape summary answers the question: *which of my managed-type
//! parameters outlive the call?*  A parameter "escapes" if it is returned,
//! stored into a heap field, captured into a returned closure, or passed to
//! another function that escapes it.
//!
//! # Structure
//!
//! [`EscapeSummary`] bundles three axes:
//!
//! - **`direct_escapes`** — parameter indices that unconditionally escape.
//! - **`conditional_escapes`** — parameters that escape *iff* a fn-typed
//!   parameter has a specific callee-parameter in its own escape set (the
//!   higher-order case, §12.0.1).
//! - **`return_aliases`** — parameters whose heap is aliased by the return
//!   value (§12.0.5).  At a call site, if the return value itself escapes,
//!   these parameters are also treated as escaping.
//!
//! # Key used in [`super::context::Context::escape_summaries`]
//!
//! Functions are keyed by their *qualified name*: a plain name for free
//! functions (e.g. `"save"`) and `ClassName_method` for methods (e.g.
//! `"Cache_store"`).  This matches the mangling convention used throughout
//! MIR lowering.

use std::collections::BTreeSet;

/// Zero-based index of a parameter in a function's parameter list.
pub type ParamIndex = usize;

/// Qualified function name used as the key in [`super::context::Context::escape_summaries`].
///
/// - Free function `save` → `"save"`
/// - Method `Cache::store` → `"Cache_store"`
pub type FunctionId = String;

/// Describes a single conditional escape: parameter `param` escapes iff the
/// fn-typed parameter at `via_fn_param` has `callee_param` in its own escape
/// set at the call site where it is resolved.
///
/// Read as: *"my parameter `param` escapes through whichever function is
/// passed as argument `via_fn_param`, specifically through that function's
/// parameter slot `callee_param`."*
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConditionalEscape {
    /// Which managed-type parameter of this function flows through a fn-typed arg.
    pub param: ParamIndex,
    /// Which fn-typed parameter carries the escape (index into the same param list).
    pub via_fn_param: ParamIndex,
    /// Which parameter slot of the fn-typed callee `param` is bound to.
    pub callee_param: ParamIndex,
}

/// Escape summary for a single function.
///
/// Computed bottom-up over the call graph by the escape analysis pass
/// (`src/type_checker/escape_analysis.rs`, §12.1).  Hand-authored entries
/// for FFI-only declarations live in `src/runtime/core/escape_summaries.toml`
/// (§12.0.2).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EscapeSummary {
    /// Parameters that unconditionally escape (returned, stored, captured,
    /// or passed to a definitely-escaping callee).
    pub direct_escapes: BTreeSet<ParamIndex>,
    /// Parameters whose escape depends on a fn-typed argument (§12.0.1).
    pub conditional_escapes: Vec<ConditionalEscape>,
    /// Parameters aliased by the return value — if the caller lets the return
    /// value escape, these params escape too (§12.0.5).
    pub return_aliases: BTreeSet<ParamIndex>,
}

impl EscapeSummary {
    /// Returns `true` if the summary is conservative-empty (nothing escapes).
    pub fn is_empty(&self) -> bool {
        self.direct_escapes.is_empty()
            && self.conditional_escapes.is_empty()
            && self.return_aliases.is_empty()
    }

    /// Returns `true` if parameter `p` unconditionally escapes.
    pub fn directly_escapes(&self, p: ParamIndex) -> bool {
        self.direct_escapes.contains(&p)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_summary_is_empty() {
        let s = EscapeSummary::default();
        assert!(s.is_empty());
        assert!(s.direct_escapes.is_empty());
        assert!(s.conditional_escapes.is_empty());
        assert!(s.return_aliases.is_empty());
    }

    #[test]
    fn direct_escape_membership() {
        let mut s = EscapeSummary::default();
        s.direct_escapes.insert(0);
        s.direct_escapes.insert(2);
        assert!(s.directly_escapes(0));
        assert!(!s.directly_escapes(1));
        assert!(s.directly_escapes(2));
        assert!(!s.is_empty());
    }

    #[test]
    fn conditional_escape_roundtrip() {
        let ce = ConditionalEscape {
            param: 0,
            via_fn_param: 1,
            callee_param: 0,
        };
        let mut s = EscapeSummary::default();
        s.conditional_escapes.push(ce.clone());
        assert_eq!(s.conditional_escapes.len(), 1);
        assert_eq!(s.conditional_escapes[0], ce);
        assert!(!s.is_empty());
    }

    #[test]
    fn return_aliases_membership() {
        let mut s = EscapeSummary::default();
        s.return_aliases.insert(1);
        assert!(s.return_aliases.contains(&1));
        assert!(!s.return_aliases.contains(&0));
    }

    #[test]
    fn equality_and_clone() {
        let mut a = EscapeSummary::default();
        a.direct_escapes.insert(0);
        let b = a.clone();
        assert_eq!(a, b);
        a.direct_escapes.insert(1);
        assert_ne!(a, b);
    }
}
