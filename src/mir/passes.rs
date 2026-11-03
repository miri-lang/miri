// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The passes run over each lowered body, each with what it declares it does
//! to the body's control-flow graph, and the check that it kept its word.
//!
//! A pass that adds, removes or redirects a block calls
//! [`Body::cfg_changed`], so a fact recorded off the body before the rewrite
//! is known to be stale. A pass that forgets would leave such a fact looking
//! current; a pass that rewrites without declaring it would hide the rewrite
//! from whoever orders the passes. Debug builds compare the body's shape
//! before and after every pass and refuse either.

use crate::mir::Body;

/// A pass run over one body at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyPass {
    pub name: &'static str,
    /// Whether the pass may add, remove or redirect a block.
    pub rewrites_cfg: bool,
}

/// Reads back gpu-resident bindings a host read needs fresh.
pub const INSERT_READBACKS: BodyPass = BodyPass {
    name: "insert_readbacks",
    rewrites_cfg: true,
};
/// Copies a value argument a call also writes.
pub const COPY_VALUE_ARGUMENTS: BodyPass = BodyPass {
    name: "copy_value_arguments",
    rewrites_cfg: false,
};
/// Converts optional operands to the layout their use expects.
pub const CONFORM_OPTIONAL_OPERANDS: BodyPass = BodyPass {
    name: "conform_optional_operands",
    rewrites_cfg: false,
};
/// Decides which parameters each function takes over from its callers.
pub const DECIDE_OWNED_PARAMETERS: BodyPass = BodyPass {
    name: "decide_owned_parameters",
    rewrites_cfg: false,
};
/// Marks the calls that release nothing their caller holds.
pub const STAMP_QUIET_CALLS: BodyPass = BodyPass {
    name: "stamp_quiet_calls",
    rewrites_cfg: false,
};
/// Inserts every retain and release.
pub const INSERT_RC: BodyPass = BodyPass {
    name: "insert_rc",
    rewrites_cfg: false,
};
/// Removes retain and release pairs that cancel.
pub const ELIDE_RC: BodyPass = BodyPass {
    name: "elide_rc",
    rewrites_cfg: false,
};
/// Drops uniqueness checks an earlier write already answered.
pub const ELIDE_UNSHARE_CHECKS: BodyPass = BodyPass {
    name: "elide_unshare_checks",
    rewrites_cfg: true,
};
/// Threads branch conditions known on an incoming edge.
pub const THREAD_BRANCH_CONDITIONS: BodyPass = BodyPass {
    name: "thread_branch_conditions",
    rewrites_cfg: true,
};
/// Copies each loop a guard on its bound proves in bounds.
pub const VERSION_BOUNDED_LOOPS: BodyPass = BodyPass {
    name: "version_bounded_loops",
    rewrites_cfg: true,
};
/// Records each collection a loop leaves unchanged.
pub const RECORD_STABLE_COLLECTIONS: BodyPass = BodyPass {
    name: "record_stable_collections",
    rewrites_cfg: true,
};
/// Records each access proven in bounds.
pub const RECORD_IN_BOUNDS: BodyPass = BodyPass {
    name: "record_in_bounds",
    rewrites_cfg: false,
};
/// Spells element arguments by address.
pub const PASS_ELEMENTS_BY_ADDRESS: BodyPass = BodyPass {
    name: "pass_elements_by_address",
    rewrites_cfg: true,
};
/// Spells closure arguments with their trampolines.
pub const PASS_CLOSURES_WITH_TRAMPOLINES: BodyPass = BodyPass {
    name: "pass_closures_with_trampolines",
    rewrites_cfg: false,
};

/// How a pass broke its declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassContract {
    /// The pass changed the graph's shape without calling
    /// [`Body::cfg_changed`].
    RewroteWithoutBumping(&'static str),
    /// The pass rewrote the graph though it declares it never does.
    RewroteUndeclared(&'static str),
}

impl std::fmt::Display for PassContract {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RewroteWithoutBumping(pass) => write!(
                f,
                "the pass `{pass}` changed the control-flow graph without recording it"
            ),
            Self::RewroteUndeclared(pass) => write!(
                f,
                "the pass `{pass}` rewrote the control-flow graph, which it declares it never does"
            ),
        }
    }
}

/// Runs `pass` over `body` through `run`, handing back what `run` returned.
///
/// In a debug build the body's shape and generation are compared before and
/// after: a changed shape needs a new generation, and a new generation needs
/// a pass that declares it rewrites the graph.
pub fn run_body_pass<R>(
    pass: &BodyPass,
    body: &mut Body,
    run: impl FnOnce(&mut Body) -> R,
) -> Result<R, PassContract> {
    if !cfg!(debug_assertions) {
        return Ok(run(body));
    }
    let shape = body.shape();
    let generation = body.generation();
    let result = run(body);
    let bumped = body.generation() != generation;
    if bumped && !pass.rewrites_cfg {
        return Err(PassContract::RewroteUndeclared(pass.name));
    }
    if !bumped && body.shape() != shape {
        return Err(PassContract::RewroteWithoutBumping(pass.name));
    }
    Ok(result)
}

/// Runs `pass` over every body of `bodies` at once, the way [`run_body_pass`]
/// runs one over a single body: a pass that reads across bodies still
/// rewrites each one on its own terms.
pub fn run_program_pass<K, R>(
    pass: &BodyPass,
    bodies: &mut [(K, Body)],
    run: impl FnOnce(&mut [(K, Body)]) -> R,
) -> Result<R, PassContract> {
    if !cfg!(debug_assertions) {
        return Ok(run(bodies));
    }
    let before: Vec<_> = (bodies.iter())
        .map(|(_, body)| (body.shape(), body.generation()))
        .collect();
    let result = run(bodies);
    for ((_, body), (shape, generation)) in bodies.iter().zip(&before) {
        let bumped = body.generation() != *generation;
        if bumped && !pass.rewrites_cfg {
            return Err(PassContract::RewroteUndeclared(pass.name));
        }
        if !bumped && body.shape() != *shape {
            return Err(PassContract::RewroteWithoutBumping(pass.name));
        }
    }
    Ok(result)
}
