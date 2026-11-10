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
//!
//! A pass also declares which fact groups ([`crate::mir::facts`]) it reads,
//! records and keeps true across its own rewrite. The runner refuses to
//! start it on a stale group it reads, stamps the groups it records, carries
//! the ones it preserves to the body's new generation, and leaves every other
//! group stale once it rewrites the graph.
//!
//! The passes that run between lowering and code generation are a list,
//! [`MIR_PASSES`], and [`PassManager`] runs it. The manager checks the list
//! before running any of it: a pass that reads a fact group needs an earlier
//! pass to record it, with no rewrite in between that leaves it stale.

use std::collections::{HashMap, HashSet};

use crate::mir::facts::{BodyFacts, Fact, ProgramFacts, StaleFact};
use crate::mir::symbol::Symbol;
use crate::mir::type_facts::DropHookReach;
use crate::mir::verify::VerificationViolation;
use crate::mir::Body;
use crate::type_checker::context::TypeDefinition;

/// A pass run over one body at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BodyPass {
    pub name: &'static str,
    /// Whether the pass may add, remove or redirect a block.
    pub rewrites_cfg: bool,
    /// The fact groups the pass reads; each must hold when it starts.
    pub requires: &'static [Fact],
    /// The fact groups the pass records; each holds once it ends.
    pub records: &'static [Fact],
    /// The fact groups a rewrite by the pass leaves true.
    pub preserves: &'static [Fact],
    /// The fact groups no pass may have recorded when it starts: the pass
    /// establishes them, and running it a second time would act on a body
    /// it already rewrote.
    pub excludes: &'static [Fact],
}

impl BodyPass {
    /// Whether the pass reads or writes any fact group.
    pub fn uses_facts(&self) -> bool {
        !(self.requires.is_empty()
            && self.records.is_empty()
            && self.preserves.is_empty()
            && self.excludes.is_empty())
    }
}

/// Releases each binding a consuming site gives up exactly once on every
/// path.
pub const ELABORATE_DROPS: BodyPass = BodyPass {
    name: "elaborate_drops",
    rewrites_cfg: true,
    requires: &[],
    records: &[Fact::DropsElaborated],
    preserves: &[],
    excludes: &[Fact::DropsElaborated],
};
/// Reads back gpu-resident bindings a host read needs fresh.
pub const INSERT_READBACKS: BodyPass = BodyPass {
    name: "insert_readbacks",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[Fact::Ownership, Fact::DropsElaborated],
    excludes: &[],
};
/// Copies a value argument a call also writes.
pub const COPY_VALUE_ARGUMENTS: BodyPass = BodyPass {
    name: "copy_value_arguments",
    rewrites_cfg: false,
    requires: &[],
    records: &[],
    preserves: &[],
    excludes: &[],
};
/// Converts optional operands to the layout their use expects.
pub const CONFORM_OPTIONAL_OPERANDS: BodyPass = BodyPass {
    name: "conform_optional_operands",
    rewrites_cfg: false,
    requires: &[],
    records: &[],
    preserves: &[],
    excludes: &[],
};
/// Decides which parameters each function takes over from its callers.
pub const DECIDE_OWNED_PARAMETERS: BodyPass = BodyPass {
    name: "decide_owned_parameters",
    rewrites_cfg: false,
    requires: &[],
    records: &[Fact::Ownership],
    preserves: &[],
    excludes: &[],
};
/// Marks the calls that release nothing their caller holds.
pub const STAMP_QUIET_CALLS: BodyPass = BodyPass {
    name: "stamp_quiet_calls",
    rewrites_cfg: false,
    requires: &[Fact::Ownership],
    records: &[],
    preserves: &[],
    excludes: &[],
};
/// Inserts every retain and release.
pub const INSERT_RC: BodyPass = BodyPass {
    name: "insert_rc",
    rewrites_cfg: false,
    requires: &[Fact::Ownership, Fact::DropsElaborated],
    records: &[Fact::Ownership],
    preserves: &[],
    excludes: &[],
};
/// Removes retain and release pairs that cancel.
pub const ELIDE_RC: BodyPass = BodyPass {
    name: "elide_rc",
    rewrites_cfg: false,
    requires: &[Fact::Ownership],
    records: &[],
    preserves: &[],
    excludes: &[],
};
/// Drops uniqueness checks an earlier write already answered.
pub const ELIDE_UNSHARE_CHECKS: BodyPass = BodyPass {
    name: "elide_unshare_checks",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[Fact::Ownership],
    excludes: &[],
};
/// Threads branch conditions known on an incoming edge.
pub const THREAD_BRANCH_CONDITIONS: BodyPass = BodyPass {
    name: "thread_branch_conditions",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[Fact::Ownership],
    excludes: &[],
};
/// Copies each loop a guard on its bound proves in bounds.
pub const VERSION_BOUNDED_LOOPS: BodyPass = BodyPass {
    name: "version_bounded_loops",
    rewrites_cfg: true,
    requires: &[Fact::Ownership],
    records: &[],
    preserves: &[Fact::Ownership],
    excludes: &[],
};
/// Records each collection a loop leaves unchanged.
pub const RECORD_STABLE_COLLECTIONS: BodyPass = BodyPass {
    name: "record_stable_collections",
    rewrites_cfg: true,
    requires: &[Fact::Ownership],
    records: &[],
    preserves: &[Fact::Ownership],
    excludes: &[],
};
/// Records each access proven in bounds.
pub const RECORD_IN_BOUNDS: BodyPass = BodyPass {
    name: "record_in_bounds",
    rewrites_cfg: false,
    requires: &[],
    records: &[],
    preserves: &[],
    excludes: &[],
};
/// Spells element arguments by address.
///
/// The locals it adds hold an address, a constant or a result slot: none
/// is a parameter or a borrow, so who owns what stays as recorded.
pub const PASS_ELEMENTS_BY_ADDRESS: BodyPass = BodyPass {
    name: "pass_elements_by_address",
    rewrites_cfg: true,
    requires: &[],
    records: &[],
    preserves: &[Fact::Ownership],
    excludes: &[],
};
/// Spells closure arguments with their trampolines.
pub const PASS_CLOSURES_WITH_TRAMPOLINES: BodyPass = BodyPass {
    name: "pass_closures_with_trampolines",
    rewrites_cfg: false,
    requires: &[],
    records: &[],
    preserves: &[],
    excludes: &[],
};

/// How a pass broke its declaration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassContract {
    /// The pass changed the graph's shape without calling
    /// [`Body::cfg_changed`].
    RewroteWithoutBumping(&'static str),
    /// The pass rewrote the graph though it declares it never does.
    RewroteUndeclared(&'static str),
    /// The pass declares fact groups but was run without the body's facts.
    RunWithoutFacts(&'static str),
    /// A fact group the pass reads did not hold when it started.
    Stale(&'static str, StaleFact),
    /// The program's facts do not describe its bodies one for one.
    FactsMisaligned { facts: usize, bodies: usize },
    /// The pass met a body it cannot rewrite soundly, and refused it rather
    /// than guess.
    Failed { pass: &'static str, reason: String },
    /// The pass establishes a fact group an earlier pass already recorded
    /// about the body.
    Repeated(&'static str, Fact),
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
            Self::RunWithoutFacts(pass) => write!(
                f,
                "the pass `{pass}` reads or records facts but was run without them"
            ),
            Self::Stale(pass, stale) => {
                write!(f, "the pass `{pass}` started on stale facts: {stale}")
            }
            Self::FactsMisaligned { facts, bodies } => write!(
                f,
                "the program holds facts for {facts} bodies but has {bodies}"
            ),
            Self::Failed { pass, reason } => {
                write!(f, "the pass `{pass}` could not complete: {reason}")
            }
            Self::Repeated(pass, fact) => write!(
                f,
                "the pass `{pass}` establishes the {fact} facts, which were already recorded"
            ),
        }
    }
}

/// Runs `pass`, which uses no facts, over `body` through `run`, handing back
/// what `run` returned.
///
/// In a debug build the body's shape and generation are compared before and
/// after: a changed shape needs a new generation, and a new generation needs
/// a pass that declares it rewrites the graph. A fact recorded off the body
/// before a rewrite run this way goes stale with the old generation.
pub fn run_body_pass<R>(
    pass: &BodyPass,
    body: &mut Body,
    run: impl FnOnce(&mut Body) -> R,
) -> Result<R, PassContract> {
    if pass.uses_facts() {
        return Err(PassContract::RunWithoutFacts(pass.name));
    }
    let watch = Watch::of(body);
    let result = run(body);
    watch.check(pass, body)?;
    Ok(result)
}

/// Runs `pass` over `body` and its `facts` through `run`, the way
/// [`run_body_pass`] runs a pass that uses none, and settles the facts after
/// it: see [`settle`].
pub fn run_body_pass_with_facts<R>(
    pass: &BodyPass,
    body: &mut Body,
    facts: &mut BodyFacts,
    run: impl FnOnce(&mut Body, &mut BodyFacts) -> R,
) -> Result<R, PassContract> {
    require(pass, body, facts)?;
    let watch = Watch::of(body);
    let result = run(body, facts);
    watch.check(pass, body)?;
    settle(pass, body, facts, watch.generation);
    Ok(result)
}

/// Runs `pass`, which uses no facts, over every body of `bodies` at once, the
/// way [`run_body_pass`] runs one over a single body: a pass that reads across
/// bodies still rewrites each one on its own terms.
pub fn run_program_pass<K, R>(
    pass: &BodyPass,
    bodies: &mut [(K, Body)],
    run: impl FnOnce(&mut [(K, Body)]) -> R,
) -> Result<R, PassContract> {
    if pass.uses_facts() {
        return Err(PassContract::RunWithoutFacts(pass.name));
    }
    let watches: Vec<_> = bodies.iter().map(|(_, body)| Watch::of(body)).collect();
    let result = run(bodies);
    for ((_, body), watch) in bodies.iter().zip(&watches) {
        watch.check(pass, body)?;
    }
    Ok(result)
}

/// Runs `pass` over every body of `bodies` and their `facts` at once, the way
/// [`run_body_pass_with_facts`] runs one over a single body.
pub fn run_program_pass_with_facts<K, R>(
    pass: &BodyPass,
    bodies: &mut [(K, Body)],
    facts: &mut ProgramFacts,
    run: impl FnOnce(&mut [(K, Body)], &mut ProgramFacts) -> R,
) -> Result<R, PassContract> {
    if facts.len() != bodies.len() {
        let (facts, bodies) = (facts.len(), bodies.len());
        return Err(PassContract::FactsMisaligned { facts, bodies });
    }
    for ((_, body), body_facts) in bodies.iter().zip(facts.iter()) {
        require(pass, body, body_facts)?;
    }
    let watches: Vec<_> = bodies.iter().map(|(_, body)| Watch::of(body)).collect();
    let result = run(bodies, facts);
    for (((_, body), body_facts), watch) in bodies.iter().zip(facts.iter_mut()).zip(&watches) {
        watch.check(pass, body)?;
        settle(pass, body, body_facts, watch.generation);
    }
    Ok(result)
}

/// Fails unless every fact group `pass` reads holds at `body`'s generation
/// and no fact group it establishes was recorded before.
fn require(pass: &BodyPass, body: &Body, facts: &BodyFacts) -> Result<(), PassContract> {
    if let Some(&fact) = pass.excludes.iter().find(|&&fact| facts.was_recorded(fact)) {
        return Err(PassContract::Repeated(pass.name, fact));
    }
    for &fact in pass.requires {
        (facts.check(fact, body)).map_err(|stale| PassContract::Stale(pass.name, stale))?;
    }
    Ok(())
}

/// Brings `facts` up to the generation `pass` left `body` at, from
/// `generation`, the one it started at: a group the pass records holds now;
/// after a rewrite, a group it preserves is carried to the new generation and
/// every other group is stale.
fn settle(pass: &BodyPass, body: &Body, facts: &mut BodyFacts, generation: u64) {
    let rewrote = body.generation() != generation;
    for fact in Fact::ALL {
        if pass.records.contains(&fact) || (rewrote && pass.preserves.contains(&fact)) {
            facts.stamp_current(fact, body);
        } else if rewrote {
            facts.invalidate(fact);
        }
    }
}

/// A body's generation, and in a debug build its shape, before a pass runs.
struct Watch {
    generation: u64,
    shape: Option<Vec<Vec<crate::mir::BasicBlock>>>,
}

impl Watch {
    fn of(body: &Body) -> Self {
        Watch {
            generation: body.generation(),
            shape: cfg!(debug_assertions).then(|| body.shape()),
        }
    }

    /// In a debug build, fails when `pass` rewrote `body` without saying so
    /// either way.
    fn check(&self, pass: &BodyPass, body: &Body) -> Result<(), PassContract> {
        let Some(shape) = &self.shape else {
            return Ok(());
        };
        let bumped = body.generation() != self.generation;
        if bumped && !pass.rewrites_cfg {
            return Err(PassContract::RewroteUndeclared(pass.name));
        }
        if !bumped && body.shape() != *shape {
            return Err(PassContract::RewroteWithoutBumping(pass.name));
        }
        Ok(())
    }
}

/// A body-at-a-time pass's work.
pub type BodyRun = fn(&mut Body, &mut BodyFacts);

/// A body pass's work when it may refuse a body it cannot rewrite soundly,
/// saying why.
pub type FallibleBodyRun = fn(&mut Body, &mut BodyFacts) -> Result<(), String>;

/// A whole-program pass's work.
pub type ProgramRun = fn(&mut [(Symbol, Body)], &mut ProgramFacts, &mut PassContext<'_>);

/// What a program pass reads beyond the bodies, and what it reports.
pub struct PassContext<'a> {
    pub definitions: &'a HashMap<String, TypeDefinition>,
    /// Which classes run a drop hook when a value of them is released.
    pub hooks: DropHookReach<'a>,
    /// The bodies called through without a MIR call naming them, such as a
    /// vtable slot's.
    pub synthesized: HashSet<Symbol>,
    /// Whether the program's MIR is verified, so a pass reports what it
    /// finds wrong across bodies.
    pub verifying: bool,
    /// What the passes found wrong across bodies, while verifying.
    pub violations: Vec<(Symbol, VerificationViolation)>,
}

impl<'a> PassContext<'a> {
    pub fn new(
        definitions: &'a HashMap<String, TypeDefinition>,
        synthesized: HashSet<Symbol>,
        verifying: bool,
    ) -> Self {
        PassContext {
            definitions,
            hooks: DropHookReach::new(definitions),
            synthesized,
            verifying,
            violations: Vec::new(),
        }
    }
}

#[derive(Clone, Copy)]
enum Run {
    Body(BodyRun),
    FallibleBody(FallibleBodyRun),
    Program(ProgramRun),
}

/// One entry of a pass list: a pass's declaration and its work.
#[derive(Clone, Copy)]
pub struct Step {
    pub pass: &'static BodyPass,
    run: Run,
}

impl Step {
    /// A pass run over each body on its own.
    pub const fn body(pass: &'static BodyPass, run: BodyRun) -> Self {
        Step {
            pass,
            run: Run::Body(run),
        }
    }

    /// A pass run over every body at once.
    pub const fn fallible_body(pass: &'static BodyPass, run: FallibleBodyRun) -> Self {
        Step {
            pass,
            run: Run::FallibleBody(run),
        }
    }

    pub const fn program(pass: &'static BodyPass, run: ProgramRun) -> Self {
        Step {
            pass,
            run: Run::Program(run),
        }
    }
}

/// How a pass list is out of order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PassOrder {
    /// `pass` reads `fact`, which no earlier pass records.
    NeverRecorded { pass: &'static str, fact: Fact },
    /// `pass` reads `fact`, which `rewrite` left stale after it was recorded.
    StaleAfter {
        pass: &'static str,
        fact: Fact,
        rewrite: &'static str,
    },
    /// `pass` establishes `fact`, which an earlier pass already recorded.
    Repeated { pass: &'static str, fact: Fact },
}

impl std::fmt::Display for PassOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NeverRecorded { pass, fact } => write!(
                f,
                "the pass `{pass}` reads the {fact} facts, which no earlier pass records"
            ),
            Self::StaleAfter {
                pass,
                fact,
                rewrite,
            } => write!(
                f,
                "the pass `{pass}` reads the {fact} facts, which `{rewrite}` left stale"
            ),
            Self::Repeated { pass, fact } => write!(
                f,
                "the pass `{pass}` establishes the {fact} facts, which an earlier pass records"
            ),
        }
    }
}

/// Runs a pass list checked to be in order.
pub struct PassManager<'s> {
    steps: &'s [Step],
}

impl<'s> PassManager<'s> {
    /// A manager for `steps`, refused unless every fact group a pass reads
    /// was recorded by an earlier pass and no rewrite since left it stale,
    /// and no pass establishes a fact group an earlier pass recorded.
    pub fn new(steps: &'s [Step]) -> Result<Self, PassOrder> {
        // For each group: `None` before any pass records it, then the pass
        // that last left it stale, if one did.
        let mut held: HashMap<Fact, Option<&'static str>> = HashMap::new();
        for step in steps {
            let pass = step.pass;
            if let Some(&fact) = pass.excludes.iter().find(|fact| held.contains_key(fact)) {
                return Err(PassOrder::Repeated {
                    pass: pass.name,
                    fact,
                });
            }
            for &fact in pass.requires {
                match held.get(&fact) {
                    None => {
                        return Err(PassOrder::NeverRecorded {
                            pass: pass.name,
                            fact,
                        })
                    }
                    Some(Some(rewrite)) => {
                        let rewrite = *rewrite;
                        return Err(PassOrder::StaleAfter {
                            pass: pass.name,
                            fact,
                            rewrite,
                        });
                    }
                    Some(None) => {}
                }
            }
            for fact in Fact::ALL {
                if pass.records.contains(&fact) {
                    held.insert(fact, None);
                } else if pass.rewrites_cfg && !pass.preserves.contains(&fact) {
                    if let Some(state) = held.get_mut(&fact) {
                        *state = Some(pass.name);
                    }
                }
            }
        }
        Ok(PassManager { steps })
    }

    /// Runs every step in order, a body step over each body before the next
    /// step starts.
    pub fn run(
        &self,
        bodies: &mut [(Symbol, Body)],
        facts: &mut ProgramFacts,
        context: &mut PassContext<'_>,
    ) -> Result<(), PassContract> {
        if facts.len() != bodies.len() {
            let (facts, bodies) = (facts.len(), bodies.len());
            return Err(PassContract::FactsMisaligned { facts, bodies });
        }
        for step in self.steps {
            match step.run {
                Run::Body(run) => {
                    for ((_, body), body_facts) in bodies.iter_mut().zip(facts.iter_mut()) {
                        run_body_pass_with_facts(step.pass, body, body_facts, run)?;
                    }
                }
                Run::FallibleBody(run) => {
                    for ((_, body), body_facts) in bodies.iter_mut().zip(facts.iter_mut()) {
                        run_body_pass_with_facts(step.pass, body, body_facts, run)?.map_err(
                            |reason| PassContract::Failed {
                                pass: step.pass.name,
                                reason,
                            },
                        )?;
                    }
                }
                Run::Program(run) => {
                    run_program_pass_with_facts(step.pass, bodies, facts, |bodies, facts| {
                        run(bodies, facts, context)
                    })?;
                }
            }
        }
        Ok(())
    }
}

/// Elaborates the drops of `body`, refusing a body that ends a binding twice
/// along one path.
fn elaborate_drops(body: &mut Body, _: &mut BodyFacts) -> Result<(), String> {
    crate::mir::drop_elaboration::elaborate_drops(body)
}

/// The passes run between lowering and the checks on the MIR the backends
/// read, in order.
///
/// Drop elaboration comes first, deciding which ends of a consumed binding
/// still release it; reference counting reads that it ran. Readbacks come next, before reference counting accounts for the host
/// arrays they detach. Ownership is decided across the program before any
/// pass reads it; retains and releases are inserted once, on the bodies as
/// lowering and the GPU passes left them, then the pairs that cancel are
/// removed. The rewrites after that see every retain spelled out.
pub const MIR_PASSES: &[Step] = &[
    Step::fallible_body(&ELABORATE_DROPS, elaborate_drops),
    Step::body(&INSERT_READBACKS, |body, _| {
        crate::mir::residency::insert_readbacks(body)
    }),
    Step::body(&COPY_VALUE_ARGUMENTS, |body, _| {
        crate::mir::value_arguments::copy_value_arguments(body)
    }),
    Step::program(&CONFORM_OPTIONAL_OPERANDS, |bodies, _, _| {
        crate::mir::optional_conformance::conform_optional_operands(bodies)
    }),
    Step::program(&DECIDE_OWNED_PARAMETERS, decide_owned_parameters),
    Step::program(&STAMP_QUIET_CALLS, |bodies, facts, context| {
        crate::mir::analysis::heap_effects::stamp_quiet_calls(bodies, facts, &context.hooks)
    }),
    Step::fallible_body(&INSERT_RC, |body, facts| {
        crate::mir::optimization::insert_rc(body, facts.ownership_mut())
    }),
    Step::body(&ELIDE_RC, |body, facts| {
        crate::mir::optimization::elide_rc(body, facts.ownership())
    }),
    Step::body(&ELIDE_UNSHARE_CHECKS, |body, _| {
        crate::mir::optimization::elide_unshare_checks(body)
    }),
    Step::body(&THREAD_BRANCH_CONDITIONS, |body, _| {
        crate::mir::optimization::thread_branch_conditions(body)
    }),
    Step::body(&VERSION_BOUNDED_LOOPS, |body, facts| {
        crate::mir::optimization::loop_versioning::version_bounded_loops(body, facts.ownership());
    }),
    Step::body(&RECORD_STABLE_COLLECTIONS, |body, facts| {
        let ownership = facts.ownership();
        crate::mir::optimization::stable_collections::record_stable_collections(body, ownership);
    }),
    Step::body(&RECORD_IN_BOUNDS, |body, _| {
        crate::mir::optimization::in_bounds::record_in_bounds(body);
    }),
];

/// Decides which parameters each function takes over, then, while
/// verifying, checks every call against the decision and the layouts its
/// arguments cross bodies in.
fn decide_owned_parameters(
    bodies: &mut [(Symbol, Body)],
    facts: &mut ProgramFacts,
    context: &mut PassContext<'_>,
) {
    use crate::mir::verify;
    let synthesized = &context.synthesized;
    crate::mir::optimization::owned_params::decide_owned_parameters(
        bodies,
        facts,
        synthesized,
        &context.hooks,
    );
    if !context.verifying {
        return;
    }
    let definitions = context.definitions;
    let violations = &mut context.violations;
    violations.extend(verify::verify_owned_argument_modes(bodies, facts));
    violations.extend(verify::verify_optional_call_layouts(bodies, definitions));
    violations.extend(verify::verify_value_call_classes(bodies, definitions));
    violations.extend(verify::verify_optional_slots(bodies, Symbol::link_name));
}
