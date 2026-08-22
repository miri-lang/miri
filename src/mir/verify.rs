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
//! | `StorageLive(x)` | `d(x) := unwritten` |
//! | `StorageDead(x)` | leak if `d(x) > 0`; then `d(x) := ⊥` |
//! | `IncRef(p)` | `d(p) += 1`, unless something downstream claims it — see below |
//! | `DecRef(p)`, `Dealloc(p)` | `d(p) -= 1`; double-release below zero, no-op while unwritten |
//! | `Assign(x, rv)` / `Reassign(x, rv)` with an owning rvalue | `d(x) += 1` |
//! | a bare-local `Move(p)` inside a `Use` or a `Cast` | `d(p) -= 1`; double-release if `p` owns none |
//! | call destination | `d(dest) += 1`, unless the callee hands back a borrow |
//! | an argument the callee takes ownership of | `d(p) -= 1` |
//! | a call that never returns | the path stops; nothing is carried into its successor |
//! | `Return` | every tracked local must own nothing |
//!
//! **A retain pays for whoever claims what it retained, and is credited where that
//! claim happens.** Aliasing lowers to `IncRef(source)` followed by
//! `Assign(dest, Copy(source))`, and the increment exists to pay for `dest`'s
//! reference: crediting `source` leaves `dest` at zero, so `dest`'s release reads as
//! a double-release while `source` reads as a leak. The claim need not be the next
//! statement — rebinding releases the old value in between — and crediting at the
//! retain rather than at the read would let that release consume the reference meant
//! for the new binding.
//!
//! **A retain feeding a slot of a value is claimed by no local.** Building an
//! aggregate, or storing through a projection, hands the reference to the value
//! being written, which releases it when it dies. The store itself consumes
//! nothing: one made without a retain is copying values the builder still owns and
//! still releases.
//!
//! **A `Copy` of a whole local with no funding retain is a borrow** and moves no
//! ownership. Reading a field or an element out is not — that produces a value of
//! its own, which its destination owns.
//!
//! **What a call does to its arguments and its result is a property of the callee**,
//! recorded per intrinsic in [`crate::runtime_fns`]. A container insertion keeps
//! what it is handed; a copy-on-write entry point takes its receiver over; a failure
//! reporter never returns; map indexing hands back a borrow of an entry the map
//! still owns. On its own [`Operand::Move`] is a uniqueness witness for
//! copy-on-write, never a statement about ownership — reading it as one consumes a
//! local at every comparison it appears in.
//!
//! **A `Cast` re-spells a type without touching the value**, so it carries across
//! whatever the read it wraps produced, and a local that keeps holding that value
//! past the cast needs a retain to pay for it.
//!
//! # Domain
//!
//! `⊥` (untracked or out of scope), `unwritten` for a local that is live but null,
//! a concrete `0..=8`, `Unbounded` for a count that grew past the cap, and
//! `Suppressed` once a finding has been reported against a local, so one defect does
//! not cascade into every block downstream. Capping is what bounds the lattice
//! height and makes the fixpoint terminate; a count that reaches the cap is a
//! reference acquired on every turn of a loop, which is itself the finding.
//!
//! Edges that disagree at a merge keep the larger count rather than collapsing, and
//! the disagreement is reported by comparing the incoming edges directly. Collapsing
//! at the merge would swallow exactly the loop case above. An edge that never wrote
//! the local disagrees with nobody: it reads null, so a release downstream frees
//! what the other edge owns and does nothing here.
//!
//! # Enabling
//!
//! `MIRI_VERIFY_MIR=warn` reports and continues; any other non-empty value, or the
//! `--verify-mir` flag, makes findings fatal.

use crate::ast::literal::Literal;
use crate::mir::operand::Operand;
use crate::mir::place::Place;
use crate::mir::rvalue::Rvalue;
use crate::mir::statement::StatementKind;
use crate::mir::terminator::TerminatorKind;
use crate::mir::{Body, ExecutionModel, Local, Statement};
use crate::runtime_fns::{diverges, hands_back_a_borrow, taken_argument_positions};
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
    /// Live but never written on this path, so the local reads as null. Releasing
    /// one is a no-op: the release path checks for null before touching anything,
    /// which is what makes rebinding a declared-but-unassigned variable work.
    Uninit,
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
            Delta::Uninit if by > 0 => Delta::Owned(by),
            Delta::Uninit => Delta::Uninit,
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
            (Delta::Uninit, other) => other,
            (owned, Delta::Uninit) => owned,
            (Delta::Owned(left), Delta::Owned(right)) => Delta::Owned(left.max(right)),
            (Delta::Suppressed, _) | (_, Delta::Suppressed) => Delta::Suppressed,
            (Delta::Unbounded, _) | (_, Delta::Unbounded) => Delta::Unbounded,
        }
    }

    /// Whether two edges arrive holding the same number of references.
    ///
    /// `⊥` disagrees with nobody: the local is not live on that edge. Neither does
    /// an unwritten one — it reads null, so a release downstream frees the value the
    /// other edge owns and does nothing on this one. Both paths stay correct, which
    /// is what a merge exists to establish. The divergence worth reporting is
    /// between edges that both wrote the local and disagree on how many references
    /// they left holding.
    fn owns_as_much_as(self, other: Delta) -> bool {
        match (self, other) {
            (Delta::Bottom, _) | (_, Delta::Bottom) => true,
            (Delta::Uninit, _) | (_, Delta::Uninit) => true,
            (left, right) => left == right,
        }
    }

    fn is_settled(self) -> bool {
        matches!(self, Delta::Bottom | Delta::Uninit | Delta::Owned(0))
    }
}

impl fmt::Display for Delta {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Delta::Bottom => write!(f, "out of scope"),
            Delta::Uninit => write!(f, "unwritten"),
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
///
/// A body that runs on a GPU has no ownership account to check. Reference
/// counting is a property of the host heap; a kernel's values live in registers
/// and in buffers the host owns and releases, and the shader backends discard
/// every RC operation. Reading such a body through the host's rules reports a
/// leak for each value the kernel holds.
pub fn verify_body(body: &Body) -> Vec<VerificationViolation> {
    if matches!(
        body.execution_model,
        ExecutionModel::GpuKernel | ExecutionModel::GpuDevice
    ) {
        return Vec::new();
    }

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

/// Blocks the analysis follows out of `bb`.
///
/// A call that never returns has no successor to carry state into, whatever block
/// the terminator names: the path stops at the call, so what the caller was still
/// holding is not a leak. The block itself stays reachable — every one of these
/// sits on a failure branch that some other edge also reaches.
fn successors_of(body: &Body, bb: usize) -> Vec<usize> {
    let Some(terminator) = body.basic_blocks[bb].terminator.as_ref() else {
        return Vec::new();
    };
    if terminator_diverges(&terminator.kind) {
        return Vec::new();
    }
    terminator
        .successors()
        .into_iter()
        .map(|block| block.0)
        .collect()
}

/// Whether a terminator hands control to a callee that never gives it back.
fn terminator_diverges(kind: &TerminatorKind) -> bool {
    match kind {
        TerminatorKind::Call { func, .. } => direct_call_name(func).is_some_and(diverges),
        TerminatorKind::VirtualCall { .. }
        | TerminatorKind::GpuLaunch { .. }
        | TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => false,
    }
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
///
/// Edges are compared by what they own, not by which state says so: a local that
/// was never written and one that was written and released both own nothing, and
/// arriving at a merge by either route is the same thing.
fn first_divergence(
    exits: &[(usize, PathState)],
    local: Local,
) -> Option<(usize, Delta, usize, Delta)> {
    let counted = |(bb, state): &(usize, PathState)| (*bb, delta_of(state, local));
    let comparable = |(_, delta): &(usize, Delta)| !matches!(delta, Delta::Suppressed);

    let (base_bb, base) = exits.iter().map(counted).find(comparable)?;
    exits
        .iter()
        .map(counted)
        .find(|(_, delta)| comparable(&(0, *delta)) && !delta.owns_as_much_as(base))
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
    let credits = retain_credits(&block.statements);

    for (index, stmt) in block.statements.iter().enumerate() {
        if credits.absorbed.contains(&index) {
            continue;
        }
        apply_statement(body, stmt, tracked, state, violations);
        if let Some(funded) = credits.funded_reads.get(&index) {
            adjust(state, tracked, *funded, 1);
        }
    }

    if let Some(terminator) = &block.terminator {
        apply_terminator(body, &terminator.kind, tracked, state, violations);
    }
}

/// Where the reference an `IncRef` pays for lands.
#[derive(Debug, Default)]
struct RetainCredits {
    /// Statement indices of retains whose reference is claimed elsewhere, so the
    /// retain itself moves no delta.
    absorbed: HashSet<usize>,
    /// Statement index of a read, and the local its retain pays for. The credit is
    /// applied where the read happens rather than where the retain does: rebinding
    /// releases the old value in between, and crediting early would let that
    /// release consume the reference meant for the new binding.
    funded_reads: HashMap<usize, Local>,
}

/// Work out, for every `IncRef` in a block, where the reference it pays for lands.
fn retain_credits(statements: &[Statement]) -> RetainCredits {
    let mut credits = RetainCredits::default();
    for (index, stmt) in statements.iter().enumerate() {
        let StatementKind::IncRef(incremented) = &stmt.kind else {
            continue;
        };
        // Retaining a field pays for the read that takes it out, and that read is
        // what credits its destination. Nothing else may claim it: crediting the
        // value holding the field would read as a reference it acquired and leaked.
        if !incremented.projection.is_empty() {
            credits.absorbed.insert(index);
            continue;
        }
        match reader_of(statements, index, incremented) {
            Some(Reader::Alias(at, local)) => {
                credits.absorbed.insert(index);
                credits.funded_reads.insert(at, local);
            }
            Some(Reader::Slot) => {
                credits.absorbed.insert(index);
            }
            None => {}
        }
    }
    credits
}

/// What claims the reference a retain pays for.
enum Reader {
    /// A read at this statement index, landing in this local.
    Alias(usize, Local),
    /// A slot inside a value, which owns the reference from then on. No tracked
    /// local holds it, so no delta moves.
    Slot,
}

/// Find what claims the reference retained at `index`, if anything does.
///
/// The reader need not be the next statement: rebinding a variable releases the old
/// value between retaining the new one and storing it, and the release of an
/// unrelated local says nothing about what this retain funds. The scan stops at
/// anything that changes what the retained place holds or owns, because past that
/// point a later read is reading a different value than the one paid for.
fn reader_of(statements: &[Statement], index: usize, retained: &Place) -> Option<Reader> {
    for (at, stmt) in statements.iter().enumerate().skip(index + 1) {
        match &stmt.kind {
            StatementKind::Assign(dest, rvalue) | StatementKind::Reassign(dest, rvalue) => {
                if stores_place_in_a_value(dest, rvalue, retained) {
                    return Some(Reader::Slot);
                }
                if reads_place_without_owning(rvalue, retained) && dest.projection.is_empty() {
                    return Some(Reader::Alias(at, dest.local));
                }
                if *dest == *retained {
                    return None;
                }
            }
            StatementKind::DecRef(place) | StatementKind::Dealloc(place) => {
                if place == retained {
                    return None;
                }
            }
            StatementKind::StorageDead(place) => {
                if place == retained {
                    return None;
                }
            }
            StatementKind::StorageLive(_) | StatementKind::IncRef(_) | StatementKind::Nop => {}
        }
    }
    None
}

/// Whether an assignment puts `place` inside a value that owns it from then on:
/// a field of an aggregate being built, or a slot of a container being written.
///
/// The reference stored there belongs to the container, which releases it when it
/// dies. No tracked local holds it, so the retain that paid for it moves no delta —
/// and an aggregate built without a retain is copying values that were already
/// owned, which is why the store itself never consumes anything.
fn stores_place_in_a_value(dest: &Place, rvalue: &Rvalue, place: &Place) -> bool {
    match rvalue {
        Rvalue::Aggregate(_, operands) => operands.iter().any(|operand| reads(operand, place)),
        Rvalue::Use(operand) => !dest.projection.is_empty() && reads(operand, place),
        Rvalue::Cast(_, _)
        | Rvalue::Ref(_)
        | Rvalue::BinaryOp(_, _, _)
        | Rvalue::UnaryOp(_, _)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(_, _)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => false,
    }
}

/// Whether an operand reads exactly `place`, however it is spelled.
fn reads(operand: &Operand, place: &Place) -> bool {
    match operand {
        Operand::Copy(source) | Operand::Move(source) => source == place,
        Operand::Constant(_) => false,
    }
}

fn apply_statement(
    body: &Body,
    stmt: &Statement,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    match &stmt.kind {
        StatementKind::StorageLive(place) => {
            if tracked.contains(&place.local) {
                state.insert(place.local, Delta::Uninit);
            }
        }
        StatementKind::StorageDead(place) => {
            flag_leak_at_scope_end(body, place, tracked, state, violations);
        }
        // A retain whose reference is claimed elsewhere never reaches here: the
        // block skips it and credits the claimant instead. What is left pays for
        // the local it names.
        StatementKind::IncRef(place) => adjust(state, tracked, place.local, 1),
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
        // Releasing a local that was never written reads null and does nothing,
        // which is how rebinding a declared-but-unassigned variable releases an
        // old value that is not there yet.
        Delta::Uninit | Delta::Unbounded | Delta::Suppressed => {}
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

fn apply_terminator(
    body: &Body,
    kind: &TerminatorKind,
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    let (func, args, destination) = match kind {
        TerminatorKind::Call {
            func,
            args,
            destination,
            ..
        } => (Some(func), args.as_slice(), destination),
        // A virtual call resolves its callee through a vtable at runtime, so
        // nothing static is known about what it takes ownership of.
        TerminatorKind::VirtualCall {
            args, destination, ..
        } => (None, args.as_slice(), destination),
        TerminatorKind::GpuLaunch {
            launch_args,
            destination,
            ..
        } => (None, launch_args.args(), destination),
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable => return,
    };

    release_taken_args(body, func, args, tracked, state, violations);

    let borrows = func
        .and_then(direct_call_name)
        .is_some_and(hands_back_a_borrow);
    if destination.projection.is_empty() && !borrows {
        adjust(state, tracked, destination.local, 1);
    }
}

/// Hand over the references a call takes ownership of.
///
/// Which arguments those are is a property of the callee, not of how the argument
/// is spelled: a `move` marks a value the caller will not read again, which is what
/// makes copy-on-write safe, but it says nothing about who releases it. Reading it
/// as ownership would consume the arguments of every call that merely builds
/// something out of what it was given.
fn release_taken_args(
    body: &Body,
    func: Option<&Operand>,
    args: &[Operand],
    tracked: &[Local],
    state: &mut PathState,
    violations: &mut Vec<VerificationViolation>,
) {
    let Some(name) = func.and_then(direct_call_name) else {
        return;
    };
    for position in taken_argument_positions(name) {
        let Some(local) = args.get(*position).and_then(bare_local_read) else {
            continue;
        };
        release(body, &Place::new(local), tracked, state, violations);
    }
}

/// The symbol a call names, for a direct call to a named function.
///
/// Indirect and closure calls resolve their callee at runtime, so nothing static
/// is known about what they take ownership of.
fn direct_call_name(func: &Operand) -> Option<&str> {
    match func {
        Operand::Constant(constant) => match &constant.literal {
            Literal::Identifier(name) => Some(name.as_str()),
            Literal::Integer(_)
            | Literal::Float(_)
            | Literal::String(_)
            | Literal::Boolean(_)
            | Literal::Regex(_)
            | Literal::None => None,
        },
        Operand::Copy(_) | Operand::Move(_) => None,
    }
}

/// Whether an rvalue reads `place` without taking over a reference to it, so that
/// a preceding retain is what pays for the destination.
fn reads_place_without_owning(rvalue: &Rvalue, place: &Place) -> bool {
    match rvalue {
        Rvalue::Use(Operand::Copy(source)) => source == place,
        Rvalue::Use(_)
        | Rvalue::Cast(_, _)
        | Rvalue::Ref(_)
        | Rvalue::Aggregate(_, _)
        | Rvalue::BinaryOp(_, _, _)
        | Rvalue::UnaryOp(_, _)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(_, _)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => false,
    }
}

/// Whether an rvalue hands its destination a reference the function must release.
fn rvalue_is_owning(rvalue: &Rvalue) -> bool {
    match rvalue {
        // A managed constant is materialized fresh and a move carries the source's
        // reference across.
        Rvalue::Use(Operand::Constant(_)) | Rvalue::Use(Operand::Move(_)) => true,
        // A cast re-spells a type without touching the value, so it carries across
        // whatever the read it wraps produced.
        Rvalue::Cast(_, _) => true,
        // An aggregate allocates, so its destination owns the reference it yields.
        Rvalue::Aggregate(_, _) => true,
        // Reading a field or element out produces a value of its own, which the
        // destination owns and releases. Copying a whole local is a borrow instead,
        // and stays one until a retain funds it — that is the alias case, where two
        // names share one value and the retain is what pays for the second name.
        Rvalue::Use(Operand::Copy(source)) => !source.projection.is_empty(),
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

/// Locals whose reference an assignment takes over.
///
/// A rebinding move, and a cast, which re-spells the type of the value it reads in
/// place rather than copying it — so the reference goes with it, and a local that
/// keeps holding the value past the cast needs a retain to pay for that.
///
/// Storing into an aggregate or a container slot is deliberately not here. What
/// pays for the stored reference is the operand's own retain, which is credited to
/// the container rather than to a local, so a store made without one is copying a
/// value that was already owned. Consuming there would report the still-rightful
/// owner's later release as one release too many.
fn consumed_bare_locals(rvalue: &Rvalue) -> Vec<Local> {
    match rvalue {
        Rvalue::Cast(operand, _) => bare_local_read(operand).into_iter().collect(),
        Rvalue::Use(_)
        | Rvalue::Ref(_)
        | Rvalue::Aggregate(_, _)
        | Rvalue::BinaryOp(_, _, _)
        | Rvalue::UnaryOp(_, _)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::MathIntrinsic(_, _)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Phi(_) => moved_bare_locals(rvalue),
    }
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
///
/// Only a plain use rebinds a value; every other rvalue reads its operands to
/// compute something and leaves them where they were. A `move` there is the
/// uniqueness witness copy-on-write relies on, not a transfer of ownership —
/// reading it as one would consume a local at every comparison it appears in.
fn moved_bare_locals(rvalue: &Rvalue) -> Vec<Local> {
    match rvalue {
        Rvalue::Use(operand) => bare_local_moved(operand).into_iter().collect(),
        Rvalue::Cast(_, _)
        | Rvalue::UnaryOp(_, _)
        | Rvalue::BinaryOp(_, _, _)
        | Rvalue::Aggregate(_, _)
        | Rvalue::MathIntrinsic(_, _)
        | Rvalue::AtomicOp { .. }
        | Rvalue::Ref(_)
        | Rvalue::Len(_)
        | Rvalue::GpuIntrinsic(_)
        | Rvalue::Phi(_) => Vec::new(),
    }
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
