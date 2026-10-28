// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! MIR optimization passes and driver.
//!
//! This module provides an iterative optimization framework for MIR transformations.
//! Passes are run in sequence until a fixpoint is reached (no pass makes changes)
//! or a maximum iteration count is exceeded.
//!
//! Perceus RC insertion is intentionally *not* part of the fixpoint loop. It runs
//! exactly once, after all optimizations are complete, via [`insert_rc`]. Running
//! Perceus inside the loop would cause duplicate IncRef/DecRef insertions and allow
//! later passes (DCE, copy propagation) to invalidate the RC annotations.
//!
//! # Available Passes
//!
//! | Pass | Description |
//! |------|-------------|
//! | [`SimplifyCfg`] | Removes empty blocks, threads unconditional jumps |
//! | [`ConstantPropagation`] | Replaces variables with known constant values |
//! | [`CopyPropagation`] | Eliminates redundant copy operations |
//! | [`DeadCodeElimination`] | Removes unused assignments and unreachable code |
//!
//! [`owned_params::decide_owned_parameters`] runs over the whole program before
//! Perceus and decides which parameters each function takes over from its
//! callers; [`elide_rc`] later cancels the retain funding such an argument
//! against the caller's release of it. [`elide_unshare_checks`] runs after
//! it, on every host body at every optimization level, and removes a
//! uniqueness check an earlier check or a freshly built value has already
//! answered. [`stable_collections::record_stable_collections`] runs last, and
//! records each array or list a loop leaves the header of unchanged, so code
//! generation reads that header once before the loop.
//! [`in_bounds::record_in_bounds`] runs after it, and records each access a
//! loop guard proves in bounds, which code generation emits unchecked.
//!
//! # Usage
//!
//! ```no_run
//! use miri::mir::optimization::{optimize, insert_rc};
//! use miri::mir::Body;
//!
//! fn example(body: &mut Body) {
//!     optimize(body);   // Runs all passes to fixpoint
//!     insert_rc(body);  // Insert RC operations exactly once, after optimizations
//! }
//! ```

pub mod branch_threading;
pub mod constant_propagation;
pub mod copy_propagation;
pub mod dead_code;
pub mod in_bounds;
pub mod index_numbering;
pub mod local_borrows;
pub mod loop_unsharing;
pub mod owned_params;
pub mod perceus;
pub mod rc_elision;
pub mod simplify_cfg;
pub mod stable_collections;
pub mod unshare_elision;

use crate::mir::{Body, ExecutionModel};
use constant_propagation::ConstantPropagation;
use copy_propagation::CopyPropagation;
use dead_code::DeadCodeElimination;
use perceus::Perceus;
use rc_elision::RcElision;
pub use rc_elision::{count_all_rc_ops, count_rc_ops};
use simplify_cfg::SimplifyCfg;
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
    /// The optimizer uses this to determine when a fixpoint has been reached.
    fn run(&mut self, body: &mut Body) -> bool;

    /// Human-readable name for this pass, used in debugging and logging.
    fn name(&self) -> &'static str;
}

/// Run all optimization passes on the MIR body until a fixpoint is reached.
///
/// Passes are applied in a fixed order (SimplifyCfg → ConstantPropagation →
/// CopyPropagation → DeadCodeElimination) and the sequence repeats until
/// no pass makes changes or `MAX_ITERATIONS` (10) is reached.
///
/// Perceus RC insertion is **not** included here. Call [`insert_rc`] after
/// this function to insert IncRef/DecRef exactly once on the optimized body.
///
/// # Arguments
///
/// * `body` - The MIR function body to optimize (mutated in place)
///
/// TODO: nothing in the compilation pipeline calls this function, and neither
/// these passes nor the SSA construction and destruction in `crate::mir::ssa`
/// are sound as written. Constant propagation treats a local as the constant
/// of any constant assignment to it and ignores later reassignments; SSA
/// renaming versions only whole-local assignments, so writes through a
/// projection and `StorageLive`/`StorageDead` keep naming the pre-SSA local.
/// Rebuild them over a proper reaching-definitions analysis, or remove them,
/// before any optimization level wires them into the pipeline.
pub fn optimize(body: &mut Body) {
    let mut passes: Vec<Box<dyn OptimizationPass>> = vec![
        Box::new(SimplifyCfg),
        Box::new(ConstantPropagation),
        Box::new(CopyPropagation),
        Box::new(DeadCodeElimination),
    ];

    let mut changed = true;
    let mut iteration = 0;
    const MAX_ITERATIONS: usize = 10;

    while changed && iteration < MAX_ITERATIONS {
        changed = false;
        iteration += 1;

        for pass in &mut passes {
            if pass.run(body) {
                changed = true;
            }
        }
    }
}

/// Insert Perceus reference-counting operations into the MIR body.
///
/// This must be called exactly once, after all optimization passes have
/// converged. Running RC insertion inside the fixpoint loop would cause
/// duplicate IncRef/DecRef and allow later passes to invalidate the RC
/// annotations.
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
