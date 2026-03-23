// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR verification pass.
//!
//! Runs after Perceus RC insertion and checks invariants that indicate
//! reference-counting bugs in the generated MIR. This is a best-effort
//! static analysis — it catches many common RC bugs but is not path-sensitive
//! and will not catch all possible violations.
//!
//! # Checks
//!
//! 1. **StorageLive/Dead balance**: For each managed local, the number of
//!    `StorageLive` events across all reachable blocks must not exceed the
//!    number of `StorageDead` events.  `StorageLive > StorageDead` indicates
//!    that the local's scope is opened but never fully closed — a potential
//!    memory leak.
//!
//!    Note: `StorageDead > StorageLive` is intentionally *not* flagged.  In
//!    branching code with early returns, cleanup blocks on multiple exclusive
//!    paths each contain a `StorageDead`, so the aggregate across the CFG
//!    legitimately exceeds the number of `StorageLive` events.
//!
//! 2. **No DecRef on parameters**: Function parameters (locals `1..=arg_count`)
//!    must never appear as the target of `DecRef`.  The caller owns the original
//!    reference; a callee-side `DecRef` would corrupt the caller's RC.
//!    `IncRef` on a parameter is allowed and expected — it fires when the
//!    parameter is copied and the copy needs its own independent reference.
//!
//! # Enabling
//!
//! The verifier is controlled by the `MIRI_VERIFY_MIR` environment variable
//! (set to any non-empty value) or the `--verify-mir` CLI flag.

use crate::mir::statement::StatementKind;
use crate::mir::{Body, Local};
use std::collections::{HashMap, HashSet};
use std::fmt;

/// A single violation detected during MIR verification.
#[derive(Debug, Clone)]
pub struct VerificationViolation {
    /// The local involved in the violation.
    pub local: Local,
    /// Human-readable name of the local (variable name or `_N` for temporaries).
    pub local_name: String,
    /// Description of the violation.
    pub message: String,
}

impl fmt::Display for VerificationViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ({}): {}", self.local, self.local_name, self.message)
    }
}

/// Verify RC invariants in a MIR body after Perceus insertion.
///
/// Returns a (possibly empty) list of violations.  When the list is non-empty
/// it indicates a compiler bug in lowering or Perceus.
///
/// Only reachable basic blocks are inspected; unreachable blocks are skipped
/// to avoid false positives from dead-code paths left behind by optimisation.
///
/// Environment-capture locals used in closure bodies are excluded from
/// StorageLive/Dead balance checking, because their lifetimes are managed
/// by the outer scope that allocated the closure environment.
pub fn verify_body(body: &Body) -> Vec<VerificationViolation> {
    let mut violations = Vec::new();

    // Closure-captured locals have their lifetimes managed by the outer scope;
    // skip them so we do not emit false-positive StorageLive/Dead imbalances.
    let env_captures: HashSet<Local> = body.env_capture_locals.iter().copied().collect();

    // Managed locals owned by this function (not parameters, not return value).
    // Local 0 = return value (no StorageLive/Dead emitted for it — skip).
    // Locals 1..=arg_count = parameters (caller-owned, Perceus skips them — skip).
    // Locals > arg_count = owned by this function — these are the ones we verify.
    let managed_locals: HashSet<Local> = body
        .local_decls
        .iter()
        .enumerate()
        .filter(|(i, decl)| {
            *i > body.arg_count
                && !env_captures.contains(&Local(*i))
                && decl
                    .mir_ty
                    .is_managed(&body.auto_copy_types, &body.type_params)
        })
        .map(|(i, _)| Local(i))
        .collect();

    // Managed parameter locals — Perceus may emit IncRef when a parameter is
    // copied (the copy needs its own reference), but must never emit DecRef
    // because the caller owns the original reference and will release it.
    let managed_params: HashSet<Local> = body
        .local_decls
        .iter()
        .enumerate()
        .filter(|(i, decl)| {
            *i >= 1
                && *i <= body.arg_count
                && decl
                    .mir_ty
                    .is_managed(&body.auto_copy_types, &body.type_params)
        })
        .map(|(i, _)| Local(i))
        .collect();

    // Only analyse reachable blocks — dead code is excluded.
    let unreachable: HashSet<usize> = body.find_unreachable_blocks().into_iter().collect();
    let reachable_blocks: Vec<usize> = (0..body.basic_blocks.len())
        .filter(|i| !unreachable.contains(i))
        .collect();

    // Collect StorageLive/Dead counts and DecRef-on-parameter violations.
    let mut storage_live_count: HashMap<Local, usize> = HashMap::new();
    let mut storage_dead_count: HashMap<Local, usize> = HashMap::new();
    let mut decref_param_violations: Vec<Local> = Vec::new();

    for bb_idx in &reachable_blocks {
        let block = &body.basic_blocks[*bb_idx];
        for stmt in &block.statements {
            match &stmt.kind {
                StatementKind::StorageLive(place) => {
                    if managed_locals.contains(&place.local) {
                        *storage_live_count.entry(place.local).or_default() += 1;
                    }
                }
                StatementKind::StorageDead(place) => {
                    if managed_locals.contains(&place.local) {
                        *storage_dead_count.entry(place.local).or_default() += 1;
                    }
                }
                StatementKind::DecRef(place) => {
                    if managed_params.contains(&place.local)
                        && !decref_param_violations.contains(&place.local)
                    {
                        decref_param_violations.push(place.local);
                    }
                }
                _ => {}
            }
        }
    }

    // --- Check 1: StorageLive not exceeded by StorageDead ---
    // We only flag `StorageLive > StorageDead` (potential leak: scope opened but
    // never fully closed).  We do NOT flag `StorageDead > StorageLive` because
    // exclusive-path cleanup in branching code (e.g. early returns) causes the
    // aggregate StorageDead count to exceed StorageLive without any bug.
    //
    // Only check locals that appear in at least one StorageLive statement;
    // skipping others avoids false positives for locals initialised through
    // Call terminators where no StorageLive statement is emitted.
    let mut sorted_locals: Vec<Local> = managed_locals.into_iter().collect();
    sorted_locals.sort_by_key(|l| l.0);
    for local in sorted_locals {
        let live = storage_live_count.get(&local).copied().unwrap_or(0);
        if live == 0 {
            continue;
        }
        let dead = storage_dead_count.get(&local).copied().unwrap_or(0);
        if live > dead {
            let name = local_display_name(body, local);
            violations.push(VerificationViolation {
                local,
                local_name: name,
                message: format!(
                    "potential leak: {} StorageLive event(s) but only {} StorageDead event(s) across all reachable paths",
                    live, dead
                ),
            });
        }
    }

    // --- Check 2: No DecRef on parameter locals ---
    // IncRef on parameters IS legal — it happens when a parameter is copied
    // (the copy needs its own reference that will be decremented at its StorageDead).
    // DecRef on parameters is NOT legal — the caller owns the reference and will
    // release it; a callee-side DecRef would corrupt the caller's reference count.
    for local in decref_param_violations {
        let name = local_display_name(body, local);
        violations.push(VerificationViolation {
            local,
            local_name: name,
            message:
                "DecRef emitted for a parameter local; parameters are caller-owned and must not be RC-managed by the callee"
                    .to_string(),
        });
    }

    violations
}

/// Returns a human-readable display name for a local: the variable name if
/// available, or `_N` for anonymous temporaries.
fn local_display_name(body: &Body, local: Local) -> String {
    body.local_decls[local.0]
        .name
        .as_ref()
        .map(|n| n.as_ref().to_string())
        .unwrap_or_else(|| format!("_{}", local.0))
}
