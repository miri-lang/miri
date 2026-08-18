// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Path-sensitive reference-counting verifier for MIR.
//!
//! Runs after Perceus insertion and RC elision. For every managed local the pass
//! tracks an *ownership delta* — how many references this function owns for that
//! local — at each program point, along each control-flow path. A reference
//! counting bug shows up as a delta that disagrees with itself: driven below zero
//! (released twice), still positive where the local dies (leaked), or holding two
//! different values on two edges into the same block (released on one path and
//! stranded on the other).
//!
//! The last of those is why the pass is path-sensitive. Counting RC events across
//! the whole body, as the previous verifier did, cannot see a release present on
//! the fall-through path and missing on the `return` path: the totals still
//! balance.
//!
//! # Transfer functions
//!
//! Derived from the MIR the pipeline actually emits. Where that disagrees with the
//! reference-counting rules stated in the abstract, the emitted form wins and the
//! reasoning is recorded here.
//!
//! | MIR event | Effect on the delta |
//! |---|---|
//! | `StorageLive(x)` | `d(x) := 0` |
//! | `StorageDead(x)` | leak if `d(x) > 0`; then `d(x) := ⊥` |
//! | `IncRef(p)` | `d(p) += 1`, unless it funds an alias — see below |
//! | `DecRef(p)`, `Dealloc(p)` | `d(p) -= 1`; double-release if it would go below zero |
//! | `Assign(x, rv)` / `Reassign(x, rv)` with an owning rvalue | `d(x) += 1` |
//! | a bare-local `Move(p)` operand inside an rvalue | `d(p) -= 1`; double-release if `p` owns none |
//! | call destination of managed type | `d(dest) += 1` |
//! | a bare-local operand of a `Cast` | `d(p) -= 1` |
//! | a bare-local `Move(p)` argument, when the call hands back that same type | `d(p) -= 1` |
//! | `Return` | every tracked local must be at `0` or `⊥` |
//!
//! **An `IncRef` funds the consumer, not the local it names.** Aliasing lowers to
//! `IncRef(source)` immediately followed by `Assign(dest, Copy(source))`, and that
//! increment exists to pay for `dest`'s reference. Crediting `source` leaves
//! `dest` at zero, so `dest`'s release reads as a double-release while `source`
//! reads as a leak. The increment is therefore attributed to the destination — but
//! only for a plain alias. When the following assignment builds an aggregate, its
//! operand increments pay for the references stored *inside* the aggregate and are
//! consumed by the `move`s that place them there, so they stay with the operands.
//!
//! **A `Copy` with no funding `IncRef` is a borrow** and moves no ownership. That
//! is why the increment is attributed rather than a reference being transferred out
//! of the source: transferring would charge a source that was only read.
//!
//! **A `Move` of a projection does not consume its base.** Field and index reads
//! are borrows by design; only a bare local carries ownership.
//!
//! **A `Move` argument is consumed only when the callee hands back the type it was
//! given.** The copy-on-write path moves its receiver in and returns the same
//! container, so it has taken that reference over; a call returning nothing is
//! mutating in place, and one returning a different type is reading its argument to
//! build something else. Both of those leave the reference with the caller. On its
//! own [`Operand::Move`] is a uniqueness witness for copy-on-write, never a
//! statement about ownership.
//!
//! **A `Cast` of a managed value re-types it rather than copying it**, so it carries
//! the source's reference across however the operand is spelled.
//!
//! # Domain
//!
//! `⊥` (untracked or out of scope), a concrete `0..=8`, `Unbounded` for a count that
//! grew past the cap, and `Suppressed` once a finding has been reported against a
//! local, so one defect does not cascade into every block downstream. Capping is
//! what bounds the lattice height and makes the fixpoint terminate; a count that
//! reaches the cap is a reference acquired on every turn of a loop, which is itself
//! the finding.
//!
//! Edges that disagree at a merge keep the larger count rather than collapsing, and
//! the disagreement is reported by comparing the incoming edges directly. Collapsing
//! at the merge would swallow exactly the loop case above.
//!
//! # Enabling
//!
//! `MIRI_VERIFY_MIR=warn` reports and continues; any other non-empty value, or the
//! `--verify-mir` flag, makes findings fatal.

use crate::mir::operand::Operand;
use crate::mir::place::Place;
use crate::mir::rvalue::Rvalue;
use crate::mir::statement::StatementKind;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, Local, Statement};
use std::collections::{HashMap, HashSet, VecDeque};
use std::fmt;

/// Largest delta the domain represents; past it a local is reported rather than
/// followed, which keeps the lattice finite.
const DELTA_CAP: i32 = 8;

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

/// References owned for one local on one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Delta {
    /// Out of scope, or not yet reached on this path.
    Bottom,
    Owned(i32),
    /// The count grew past [`DELTA_CAP`] — a reference acquired on every turn of a
    /// loop and never released. Reported rather than followed, which is also what
    /// bounds the lattice and makes the fixpoint terminate.
    Unbounded,
    /// A finding was already reported against this local; further ones would be
    /// consequences of it rather than separate defects.
    Suppressed,
}

impl Delta {
    fn shift(self, by: i32) -> Delta {
        match self {
            Delta::Bottom => Delta::Owned(by.max(0)),
            Delta::Owned(n) if (0..=DELTA_CAP).contains(&(n + by)) => Delta::Owned(n + by),
            Delta::Owned(_) => Delta::Unbounded,
            Delta::Unbounded => Delta::Unbounded,
            Delta::Suppressed => Delta::Suppressed,
        }
    }

    /// `⊥` is the identity: the local is simply not live on that edge.
    ///
    /// Edges that disagree keep the larger count rather than collapsing to `⊤`.
    /// The disagreement is itself reported, by comparing the incoming edges
    /// directly, and a concrete value keeps the blocks downstream analysable —
    /// collapsing here would swallow a reference that grows around a loop, since
    /// every later state would already be `⊤`.
    fn join(self, other: Delta) -> Delta {
        match (self, other) {
            (Delta::Bottom, other) => other,
            (owned, Delta::Bottom) => owned,
            (Delta::Owned(left), Delta::Owned(right)) => Delta::Owned(left.max(right)),
            (Delta::Suppressed, _) | (_, Delta::Suppressed) => Delta::Suppressed,
            (Delta::Unbounded, _) | (_, Delta::Unbounded) => Delta::Unbounded,
        }
    }

    fn is_settled(self) -> bool {
        matches!(self, Delta::Bottom | Delta::Owned(0))
    }
}

impl fmt::Display for Delta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Delta::Bottom => write!(f, "out of scope"),
            Delta::Owned(n) => write!(f, "{}", n),
            Delta::Unbounded => write!(f, "more than {}", DELTA_CAP),
            Delta::Suppressed => write!(f, "indeterminate"),
        }
    }
}

/// Ownership deltas for every tracked local at one program point.
type PathState = HashMap<Local, Delta>;

fn delta_of(state: &PathState, local: Local) -> Delta {
    state.get(&local).copied().unwrap_or(Delta::Bottom)
}

fn join_states(into: &mut PathState, from: &PathState) {
    for (local, delta) in from {
        let merged = delta_of(into, *local).join(*delta);
        into.insert(*local, merged);
    }
}

/// Verify RC invariants in a MIR body after Perceus insertion and RC elision.
///
/// Returns a (possibly empty) list of violations. A non-empty list indicates a bug
/// in lowering or in one of the RC passes, not in the program being compiled.
pub fn verify_body(body: &Body) -> Vec<VerificationViolation> {
    let env_captures: HashSet<Local> = body.env_capture_locals.iter().copied().collect();
    let tracked = collect_tracked_locals(body, &env_captures);
    let managed_params = collect_managed_param_locals(body);
    let reachable = reachable_block_indices(body);

    let mut violations = Vec::new();
    flag_decref_on_params(body, &managed_params, &mut violations);

    let entries = run_to_fixpoint(body, &tracked, &reachable);
    report_path_findings(body, &tracked, &reachable, &entries, &mut violations);
    report_join_divergences(body, &tracked, &reachable, &entries, &mut violations);
    violations
}

/// Locals whose ownership this function is responsible for: managed, not the
/// return slot, not a parameter, and not an environment capture — a closure's
/// captures are released by its destructor, not by the body that reads them.
fn collect_tracked_locals(body: &Body, env_captures: &HashSet<Local>) -> Vec<Local> {
    body.local_decls
        .iter()
        .enumerate()
        .filter(|(index, decl)| {
            *index > body.arg_count
                && !env_captures.contains(&Local(*index))
                && decl
                    .mir_ty
                    .is_managed(&body.unmanaged_type_names, &body.type_params)
        })
        .map(|(index, _)| Local(index))
        .collect()
}

/// Managed parameter locals — caller-owned, must never receive a callee-side DecRef.
fn collect_managed_param_locals(body: &Body) -> HashSet<Local> {
    body.local_decls
        .iter()
        .enumerate()
        .filter(|(index, decl)| {
            *index >= 1
                && *index <= body.arg_count
                && decl
                    .mir_ty
                    .is_managed(&body.unmanaged_type_names, &body.type_params)
        })
        .map(|(index, _)| Local(index))
        .collect()
}

fn reachable_block_indices(body: &Body) -> Vec<usize> {
    let unreachable: HashSet<usize> = body.find_unreachable_blocks().into_iter().collect();
    (0..body.basic_blocks.len())
        .filter(|index| !unreachable.contains(index))
        .collect()
}

/// IncRef on parameters is legal (a callee-side copy needs its own reference);
/// DecRef on parameters corrupts the caller's reference count and is rejected.
fn flag_decref_on_params(
    body: &Body,
    managed_params: &HashSet<Local>,
    violations: &mut Vec<VerificationViolation>,
) {
    let mut seen: Vec<Local> = Vec::new();
    for block in &body.basic_blocks {
        for stmt in &block.statements {
            if let StatementKind::DecRef(place) = &stmt.kind {
                if managed_params.contains(&place.local) && !seen.contains(&place.local) {
                    seen.push(place.local);
                }
            }
        }
    }
    seen.sort_by_key(|local| local.0);
    for local in seen {
        violations.push(VerificationViolation {
            local,
            local_name: local_display_name(body, local),
            message:
                "DecRef emitted for a parameter local; parameters are caller-owned and must not be RC-managed by the callee"
                    .to_string(),
        });
    }
}

/// Forward dataflow to a fixpoint, returning each reachable block's entry state.
///
/// Findings are not raised here: a block is re-analysed whenever a predecessor
/// changes, so reporting mid-iteration would emit the same finding repeatedly and
/// report states that later iterations correct.
fn run_to_fixpoint(
    body: &Body,
    tracked: &[Local],
    reachable: &[usize],
) -> HashMap<usize, PathState> {
    let mut entries: HashMap<usize, PathState> = HashMap::new();
    let mut analysed: HashSet<usize> = HashSet::new();
    let mut worklist: VecDeque<usize> = VecDeque::new();

    if reachable.contains(&0) {
        entries.insert(0, PathState::new());
        worklist.push_back(0);
    }

    while let Some(bb) = worklist.pop_front() {
        analysed.insert(bb);
        let mut state = entries.get(&bb).cloned().unwrap_or_default();
        run_block(body, bb, tracked, &mut state, &mut Vec::new());

        for successor in successors_of(body, bb) {
            if !reachable.contains(&successor) {
                continue;
            }
            let entry = entries.entry(successor).or_default();
            let before = entry.clone();
            join_states(entry, &state);
            let changed = *entry != before;
            if (changed || !analysed.contains(&successor)) && !worklist.contains(&successor) {
                worklist.push_back(successor);
            }
        }
    }

    entries.retain(|bb, _| reachable.contains(bb));
    entries
}

fn successors_of(body: &Body, bb: usize) -> Vec<usize> {
    body.basic_blocks[bb]
        .terminator
        .as_ref()
        .map(|terminator| {
            terminator
                .successors()
                .into_iter()
                .map(|block| block.0)
                .collect()
        })
        .unwrap_or_default()
}

/// Exit state of a block, recomputed from its converged entry state.
fn exit_state_of(
    body: &Body,
    bb: usize,
    tracked: &[Local],
    entries: &HashMap<usize, PathState>,
) -> PathState {
    let mut state = entries.get(&bb).cloned().unwrap_or_default();
    run_block(body, bb, tracked, &mut state, &mut Vec::new());
    state
}

/// Re-run each block over its converged entry state, this time raising findings.
fn report_path_findings(
    body: &Body,
    tracked: &[Local],
    reachable: &[usize],
    entries: &HashMap<usize, PathState>,
    violations: &mut Vec<VerificationViolation>,
) {
    for bb in reachable {
        let mut state = entries.get(bb).cloned().unwrap_or_default();
        run_block(body, *bb, tracked, &mut state, violations);

        let returns = body.basic_blocks[*bb]
            .terminator
            .as_ref()
            .is_some_and(|terminator| matches!(terminator.kind, TerminatorKind::Return));
        if !returns {
            continue;
        }
        for local in tracked {
            let delta = delta_of(&state, *local);
            if delta.is_settled() || delta == Delta::Suppressed {
                continue;
            }
            violations.push(VerificationViolation {
                local: *local,
                local_name: local_display_name(body, *local),
                message: format!(
                    "still owns {} reference(s) at the return in bb{}; the release is missing on this path",
                    delta, bb
                ),
            });
        }
    }
}

/// A local reaching a merge with different deltas on two edges was released on one
/// path and stranded on the other — the shape every reference-counting seam bug
/// takes, and the reason this analysis is path-sensitive.
fn report_join_divergences(
    body: &Body,
    tracked: &[Local],
    reachable: &[usize],
    entries: &HashMap<usize, PathState>,
    violations: &mut Vec<VerificationViolation>,
) {
    for bb in reachable {
        let predecessors: Vec<usize> = reachable
            .iter()
            .copied()
            .filter(|candidate| successors_of(body, *candidate).contains(bb))
            .collect();
        if predecessors.len() < 2 {
            continue;
        }

        let exits: Vec<(usize, PathState)> = predecessors
            .iter()
            .map(|pred| (*pred, exit_state_of(body, *pred, tracked, entries)))
            .collect();

        for local in tracked {
            if let Some(divergence) = first_divergence(&exits, *local) {
                let (base_bb, base, other_bb, other) = divergence;
                violations.push(VerificationViolation {
                    local: *local,
                    local_name: local_display_name(body, *local),
                    message: format!(
                        "ownership diverges entering bb{}: bb{} owns {}, bb{} owns {}",
                        bb, base_bb, base, other_bb, other
                    ),
                });
            }
        }
    }
}

/// The first pair of in-edges that disagree about how many references they own.
fn first_divergence(
    exits: &[(usize, PathState)],
    local: Local,
) -> Option<(usize, Delta, usize, Delta)> {
    let (base_bb, base) = exits
        .iter()
        .map(|(bb, state)| (*bb, delta_of(state, local)))
        .find(|(_, delta)| !matches!(delta, Delta::Bottom | Delta::Suppressed))?;

    exits
        .iter()
        .map(|(bb, state)| (*bb, delta_of(state, local)))
        .find(|(_, delta)| !matches!(delta, Delta::Bottom | Delta::Suppressed) && *delta != base)
        .map(|(other_bb, other)| (base_bb, base, other_bb, other))
}

/// Apply every statement and then the terminator of one block to `state`.
fn run_block(
    body: &Body,
    bb: usize,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    let block = &body.basic_blocks[bb];
    let funded = aliases_funded_by_incref(&block.statements);

    for (index, stmt) in block.statements.iter().enumerate() {
        let credit = funded.get(&index).copied();
        apply_statement(body, stmt, credit, tracked, state, violations);
    }

    if let Some(terminator) = &block.terminator {
        apply_terminator(body, &terminator.kind, tracked, state);
    }
}

/// Map each `IncRef` statement index to the local its increment actually funds.
///
/// An increment immediately followed by an assignment that reads the same place by
/// `Copy` pays for the destination's reference, so the destination is credited.
fn aliases_funded_by_incref(statements: &[Statement]) -> HashMap<usize, Local> {
    let mut funded = HashMap::new();
    for (index, stmt) in statements.iter().enumerate() {
        let StatementKind::IncRef(incremented) = &stmt.kind else {
            continue;
        };
        let Some(next) = statements.get(index + 1) else {
            continue;
        };
        let (dest, rvalue) = match &next.kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                (dest, rvalue)
            }
            StatementKind::StorageLive(_)
            | StatementKind::StorageDead(_)
            | StatementKind::IncRef(_)
            | StatementKind::DecRef(_)
            | StatementKind::Dealloc(_)
            | StatementKind::Nop => continue,
        };
        if let Rvalue::Use(Operand::Copy(source)) = rvalue {
            if source == incremented && dest.projection.is_empty() {
                funded.insert(index, dest.local);
            }
        }
    }
    funded
}

fn apply_statement(
    body: &Body,
    stmt: &Statement,
    incref_credit: Option<Local>,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    match &stmt.kind {
        StatementKind::StorageLive(place) => {
            if tracked.contains(&place.local) {
                state.insert(place.local, Delta::Owned(0));
            }
        }
        StatementKind::StorageDead(place) => {
            flag_leak_at_scope_end(body, place, tracked, state, violations);
        }
        StatementKind::IncRef(place) => {
            let credited = incref_credit.unwrap_or(place.local);
            adjust(state, tracked, credited, 1);
        }
        StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
            release(body, place, tracked, state, violations);
        }
        StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
            if dest.projection.is_empty() && rvalue_is_owning(rvalue) {
                adjust(state, tracked, dest.local, 1);
            }
            for consumed in consumed_bare_locals(rvalue) {
                release(body, &Place::new(consumed), tracked, state, violations);
            }
        }
        StatementKind::Nop => {}
    }
}

fn flag_leak_at_scope_end(
    body: &Body,
    place: &Place,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    if !tracked.contains(&place.local) {
        return;
    }
    let delta = delta_of(state, place.local);
    if !delta.is_settled() && delta != Delta::Suppressed {
        violations.push(VerificationViolation {
            local: place.local,
            local_name: local_display_name(body, place.local),
            message: format!(
                "leaks {} reference(s): still owned where it goes out of scope",
                delta
            ),
        });
    }
    state.insert(place.local, Delta::Bottom);
}

fn adjust(state: &mut PathState, tracked: &[Local], local: Local, by: i32) {
    if !tracked.contains(&local) {
        return;
    }
    let shifted = delta_of(state, local).shift(by);
    state.insert(local, shifted);
}

fn release(
    body: &Body,
    place: &Place,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    if !tracked.contains(&place.local) || !place.projection.is_empty() {
        return;
    }
    match delta_of(state, place.local) {
        Delta::Owned(owned) if owned > 0 => {
            state.insert(place.local, Delta::Owned(owned - 1));
        }
        Delta::Unbounded | Delta::Suppressed => {}
        Delta::Bottom | Delta::Owned(_) => {
            violations.push(VerificationViolation {
                local: place.local,
                local_name: local_display_name(body, place.local),
                message: "released more references than it owns (double-release)".to_string(),
            });
            state.insert(place.local, Delta::Suppressed);
        }
    }
}

fn apply_terminator(body: &Body, kind: &TerminatorKind, tracked: &[Local], state: &mut PathState) {
    let (args, destination) = match kind {
        TerminatorKind::Call {
            args, destination, ..
        }
        | TerminatorKind::VirtualCall {
            args, destination, ..
        } => (args.as_slice(), destination),
        TerminatorKind::GpuLaunch {
            launch_args,
            destination,
            ..
        } => (launch_args.args(), destination),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => return,
    };

    if !destination.projection.is_empty() || !tracked.contains(&destination.local) {
        return;
    }
    // A callee handing back the type it was given has taken that argument over —
    // the copy-on-write path moves its receiver in and returns the container. One
    // returning a different type is reading the argument and building something
    // else, so the caller keeps its reference.
    let handed_back = &body.local_decls[destination.local.0].mir_ty;
    for arg in args {
        let Some(local) = bare_local_moved(arg) else {
            continue;
        };
        if body.local_decls[local.0].mir_ty == *handed_back {
            adjust(state, tracked, local, -1);
        }
    }
    adjust(state, tracked, destination.local, 1);
}

/// Whether an rvalue hands its destination a reference the function must release.
fn rvalue_is_owning(rvalue: &Rvalue) -> bool {
    match rvalue {
        // A managed constant is materialized fresh and a move carries the source's
        // reference across.
        Rvalue::Use(Operand::Constant(_)) | Rvalue::Use(Operand::Move(_)) => true,
        // Aggregates allocate; a cast of a managed value and an explicit allocation
        // both yield a reference the destination owns.
        Rvalue::Aggregate(_, _) | Rvalue::Cast(_, _) | Rvalue::Allocate(_, _, _) => true,
        // A plain copy is a borrow until an IncRef funds it.
        Rvalue::Use(Operand::Copy(_)) => false,
        Rvalue::Ref(_)
        | Rvalue::BinaryOp(_, _, _)
        | Rvalue::UnaryOp(_, _)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(_, _)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => false,
    }
}

fn bare_local_moved(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Move(place) if place.projection.is_empty() => Some(place.local),
        Operand::Move(_) | Operand::Copy(_) | Operand::Constant(_) => None,
    }
}

/// Locals whose reference an rvalue takes over.
///
/// A cast of a managed value re-types it in place rather than copying it, so it
/// carries the source's reference across however the operand is spelled; every
/// other rvalue takes over only what is moved into it.
fn consumed_bare_locals(rvalue: &Rvalue) -> Vec<Local> {
    if let Rvalue::Cast(operand, _) = rvalue {
        return bare_local_read(operand).into_iter().collect();
    }
    moved_bare_locals(rvalue)
}

/// The bare local an operand reads, whether it is spelled as a copy or a move.
fn bare_local_read(operand: &Operand) -> Option<Local> {
    match operand {
        Operand::Move(place) | Operand::Copy(place) if place.projection.is_empty() => {
            Some(place.local)
        }
        Operand::Move(_) | Operand::Copy(_) | Operand::Constant(_) => None,
    }
}

/// Locals whose reference an rvalue takes over by moving them out.
fn moved_bare_locals(rvalue: &Rvalue) -> Vec<Local> {
    let operands: Vec<&Operand> = match rvalue {
        Rvalue::Use(operand) => vec![operand],
        Rvalue::Cast(operand, _) | Rvalue::UnaryOp(_, operand) => vec![operand.as_ref()],
        Rvalue::BinaryOp(_, left, right) => vec![left.as_ref(), right.as_ref()],
        Rvalue::Aggregate(_, operands) | Rvalue::MathIntrinsic(_, operands) => {
            operands.iter().collect()
        }
        Rvalue::Allocate(size, align, allocator) => vec![size, align, allocator],
        Rvalue::AtomicOp {
            op: _,
            buffer,
            index,
            value,
            compare_expected,
        } => {
            let mut operands = vec![buffer.as_ref(), index.as_ref(), value.as_ref()];
            if let Some(expected) = compare_expected {
                operands.push(expected.as_ref());
            }
            operands
        }
        // A `Phi` merges values the incoming edges already accounted for; taking
        // every operand here would charge each of them for the one that is selected.
        Rvalue::Ref(_) | Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) | Rvalue::Phi(_) => Vec::new(),
    };
    operands.into_iter().filter_map(bare_local_moved).collect()
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
