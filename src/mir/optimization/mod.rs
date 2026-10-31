// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR-to-MIR passes over lowered bodies.
//!
//! Perceus RC insertion runs exactly once per body via [`insert_rc`]; every
//! other pass here reads or rewrites the reference-counted body it leaves.
//!
//! [`owned_params::decide_owned_parameters`] runs over the whole program before
//! Perceus and decides which parameters each function takes over from its
//! callers; [`elide_rc`] later cancels the retain funding such an argument
//! against the caller's release of it. [`elide_unshare_checks`] runs after
//! it, on every host body at every optimization level, and removes a
//! uniqueness check an earlier check or a freshly built value has already
//! answered. [`loop_versioning::version_bounded_loops`] runs next, and gives
//! a loop compared with a bound other than a length a copy entered only when
//! a guard ties the bound to the length of each list it indexes.
//! [`stable_collections::record_stable_collections`] runs after it, and
//! records each array or list a loop leaves the header of unchanged, so code
//! generation reads that header once before the loop.
//! [`in_bounds::record_in_bounds`] runs after it, and records each access a
//! loop guard proves in bounds, which code generation emits unchecked.

pub mod branch_threading;
pub mod in_bounds;
pub mod index_numbering;
mod lent_arguments;
pub mod local_borrows;
pub mod loop_unsharing;
pub mod loop_versioning;
pub mod owned_params;
pub mod perceus;
pub mod rc_elision;
pub mod stable_collections;
pub mod unshare_elision;

use crate::mir::{Body, ExecutionModel};
use perceus::Perceus;
use rc_elision::RcElision;
pub use rc_elision::{count_all_rc_ops, count_rc_ops};
use unshare_elision::UnshareElision;

/// Defines an optimization pass for MIR transformations.
///
/// Optimization passes are stateless transformations applied to MIR function bodies.
/// Each pass should focus on a single optimization strategy.
///
/// # Implementing a Pass
///
/// 1. Create a unit struct for your pass
/// 2. Implement `run()` to perform the transformation
/// 3. Return `true` from `run()` if any modifications were made
/// 4. Implement `name()` for debugging and logging
///
/// # Example
///
/// ```no_run
/// use miri::mir::Body;
/// use miri::mir::optimization::OptimizationPass;
///
/// pub struct MyOptimization;
///
/// impl OptimizationPass for MyOptimization {
///     fn run(&mut self, body: &mut Body) -> bool {
///         let mut changed = false;
///         // ... perform transformations ...
///         changed
///     }
///
///     fn name(&self) -> &'static str {
///         "My Optimization"
///     }
/// }
/// ```
pub trait OptimizationPass {
    /// Apply the optimization pass to the function body.
    ///
    /// Returns `true` if any modifications were made to the body.
    fn run(&mut self, body: &mut Body) -> bool;

    /// Human-readable name for this pass, used in debugging and logging.
    fn name(&self) -> &'static str;
}

/// Insert Perceus reference-counting operations into the MIR body.
///
/// This must be called exactly once per body: a second run would insert
/// duplicate IncRef/DecRef.
///
/// A body that runs on a GPU is left alone. There is no heap to count
/// references into: every value a kernel holds lives in a register or in a
/// buffer the host owns, and the shader backends discard `IncRef`/`DecRef`
/// outright. Annotating one produces operations no backend reads and an
/// ownership account that describes nothing.
///
/// # Arguments
///
/// * `body` - The MIR function body to annotate with RC operations (mutated in place)
pub fn insert_rc(body: &mut Body) {
    if runs_on_a_gpu(body) {
        return;
    }
    Perceus.run(body);
}

/// Whether `body` executes on a GPU rather than on the host.
fn runs_on_a_gpu(body: &Body) -> bool {
    matches!(
        body.execution_model,
        ExecutionModel::GpuKernel | ExecutionModel::GpuDevice
    )
}

/// Remove redundant IncRef/DecRef pairs from the MIR body.
///
/// Must be called after [`insert_rc`]. Runs the RC elision pass which removes
/// balanced `(IncRef(src), DecRef(dest))` pairs where `dest = Copy(src)` flows
/// linearly with no aliasing hazards, and cancels the retain funding an owned
/// argument against the caller's release of a local dying at the call
/// ([`rc_elision::cancel_owned_transfers`]).
///
/// This pass is idempotent: running it twice produces the same result.
///
/// # Arguments
///
/// * `body` - The MIR function body already annotated with RC operations
pub fn elide_rc(body: &mut Body) {
    if runs_on_a_gpu(body) {
        return;
    }
    RcElision.run(body);
}

/// Let each block branch on the condition it computed rather than jump to a
/// join block that branches on a copy of it ([`branch_threading`]). A body
/// that runs on a GPU is left alone: its backend rebuilds structured control
/// flow from the blocks as lowering left them.
pub fn thread_branch_conditions(body: &mut Body) {
    if runs_on_a_gpu(body) {
        return;
    }
    branch_threading::thread_branch_conditions(body);
}

/// Remove each uniqueness check a write path repeats after an earlier check or
/// a fresh value has already settled that nothing else holds the place.
///
/// Computed indices holding one value are first named alike
/// ([`index_numbering::number_indices`]), so two writes through `l[i + 1]`
/// reach one place, and a collection every iteration of a loop asks to copy is
/// made its own once before the loop ([`loop_unsharing::hoist_loop_unsharing`]),
/// so the questions inside the loop can be answered.
///
/// Must be called after [`elide_rc`], on bodies whose reference counting is
/// explicit, so every retain that could share a place again is visible.
/// A body that runs on a GPU is left alone; it never checks.
///
/// # Arguments
///
/// * `body` - The MIR function body already annotated with RC operations
pub fn elide_unshare_checks(body: &mut Body) {
    if runs_on_a_gpu(body) {
        return;
    }
    index_numbering::number_indices(body);
    loop_unsharing::hoist_loop_unsharing(body);
    UnshareElision.run(body);
}
