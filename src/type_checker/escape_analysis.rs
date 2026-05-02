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
//!
//! # FFI summaries (§12.0.2)
//!
//! Escape summaries for `runtime "core" fn` declarations (FFI-only, no body)
//! are hand-authored in `src/runtime/core/escape_summaries.toml` and loaded
//! at startup via [`load_ffi_summaries`].  The TOML is embedded into the
//! compiler binary with `include_str!`.

use std::collections::{BTreeSet, HashMap};

use serde::Deserialize;

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
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
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

// ── TOML deserialization helpers ──────────────────────────────────────────────

/// Serde-friendly intermediate form for one entry in `escape_summaries.toml`.
/// Uses `Vec` rather than `BTreeSet` because TOML arrays map naturally to Vec.
#[derive(Debug, Deserialize)]
struct TomlSummaryEntry {
    #[serde(default)]
    direct_escapes: Vec<ParamIndex>,
    #[serde(default)]
    return_aliases: Vec<ParamIndex>,
    #[serde(default)]
    conditional_escapes: Vec<ConditionalEscape>,
}

impl From<TomlSummaryEntry> for EscapeSummary {
    fn from(e: TomlSummaryEntry) -> Self {
        EscapeSummary {
            direct_escapes: e.direct_escapes.into_iter().collect(),
            conditional_escapes: e.conditional_escapes,
            return_aliases: e.return_aliases.into_iter().collect(),
        }
    }
}

/// Loads the hand-authored FFI escape summaries from the embedded TOML sidecar
/// (`src/runtime/core/escape_summaries.toml`, §12.0.2).
///
/// The TOML is embedded at compile time; no filesystem access occurs at
/// runtime.  Returns an empty map if parsing fails (panics via `debug_assert!`
/// in debug builds, silent in release) — this is safe because the escape
/// analysis falls back to the conservative "all managed params escape" default
/// for any unknown function.  In practice the embedded TOML is validated by
/// `load_ffi_summaries_parses_without_panic` before release.
pub fn load_ffi_summaries() -> HashMap<FunctionId, EscapeSummary> {
    const TOML_SRC: &str = include_str!("../runtime/core/escape_summaries.toml");

    match toml::from_str::<HashMap<FunctionId, TomlSummaryEntry>>(TOML_SRC) {
        Ok(raw) => raw.into_iter().map(|(k, v)| (k, v.into())).collect(),
        Err(e) => {
            // Unreachable in normal builds — the TOML is embedded and tested.
            // In debug mode, surface the error so it is caught early.
            debug_assert!(false, "failed to parse escape_summaries.toml: {e}");
            HashMap::new()
        }
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

    // ── FFI summary loading tests (§12.0.2) ───────────────────────────────────

    #[test]
    fn load_ffi_summaries_parses_without_panic() {
        let summaries = load_ffi_summaries();
        assert!(
            !summaries.is_empty(),
            "escape_summaries.toml should have at least one entry"
        );
    }

    #[test]
    fn list_push_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_list_push")
            .expect("miri_rt_list_push must have a summary");
        // param 1 (val) escapes into the list
        assert!(s.directly_escapes(1), "val (param 1) must escape");
        // param 0 (the raw list pointer) is unmanaged — not listed
        assert!(!s.directly_escapes(0));
        assert!(s.conditional_escapes.is_empty());
        assert!(s.return_aliases.is_empty());
    }

    #[test]
    fn map_set_escapes_key_and_value() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_map_set")
            .expect("miri_rt_map_set must have a summary");
        assert!(s.directly_escapes(1), "key (param 1) must escape");
        assert!(s.directly_escapes(2), "value (param 2) must escape");
        assert!(!s.directly_escapes(0));
    }

    #[test]
    fn set_add_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_set_add")
            .expect("miri_rt_set_add must have a summary");
        assert!(s.directly_escapes(1), "elem (param 1) must escape");
        assert!(!s.directly_escapes(0));
    }

    #[test]
    fn io_sinks_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &[
            "miri_rt_print",
            "miri_rt_println",
            "miri_rt_eprint",
            "miri_rt_eprintln",
        ] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is an IO sink — no parameters should escape"
            );
        }
    }

    #[test]
    fn list_insert_and_set_escape_element() {
        let summaries = load_ffi_summaries();
        for name in &["miri_rt_list_insert", "miri_rt_list_set"] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have a summary"));
            assert!(s.directly_escapes(2), "{name}: val (param 2) must escape");
        }
    }

    #[test]
    fn array_set_val_escapes_element() {
        let summaries = load_ffi_summaries();
        let s = summaries
            .get("miri_rt_array_set_val")
            .expect("miri_rt_array_set_val must have a summary");
        assert!(s.directly_escapes(2), "val (param 2) must escape");
        assert!(!s.directly_escapes(0));
        assert!(!s.directly_escapes(1));
    }

    #[test]
    fn map_read_only_accessors_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &[
            "miri_rt_map_get",
            "miri_rt_map_contains_key",
            "miri_rt_map_remove",
        ] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is a read-only accessor — no parameters should escape"
            );
        }
    }

    #[test]
    fn set_read_only_accessors_have_no_escapes() {
        let summaries = load_ffi_summaries();
        for name in &["miri_rt_set_contains", "miri_rt_set_remove"] {
            let s = summaries
                .get(*name)
                .unwrap_or_else(|| panic!("{name} must have an explicit summary"));
            assert!(
                s.is_empty(),
                "{name} is a read-only accessor — no parameters should escape"
            );
        }
    }
}
