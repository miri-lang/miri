// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The GPU copies of the functions GPU kernels call.

use std::collections::{HashMap, HashSet};

use crate::ast::types::TypeKind;
use crate::diagnostics::DiagnosticCode;
use crate::error::lowering::LoweringError;
use crate::error::syntax::Span;
use crate::mir::call_graph::CalleeWalk;
use crate::mir::gpu_names::GpuNames;
use crate::mir::place_type::PlaceTypes;
use crate::mir::representation::{out_handover, OutHandover, Representation};
use crate::mir::symbol::Symbol;
use crate::mir::types::MirType;
use crate::mir::{
    ArgMode, BasicBlock, BasicBlockData, Body, Discriminant, ExecutionModel, LocalDecl, Operand,
    Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};

/// Clone user functions that are transitively called from GPU kernels.
/// Each clone is marked as GpuDevice and has f32-narrowed types.
///
/// A clone keeps the symbol of the body it was cloned from, so afterwards a
/// helper's symbol names two bodies: the CPU original and its GpuDevice
/// clone. They are told apart by execution model, which is what every later
/// lookup by symbol selects on.
pub fn clone_gpu_device_helpers(
    bodies: &mut Vec<(Symbol, Body)>,
    gpu_names: &mut GpuNames,
) -> Result<(), LoweringError> {
    let body_of: HashMap<&Symbol, &Body> =
        bodies.iter().map(|(symbol, body)| (symbol, body)).collect();
    let kernels: HashSet<&Symbol> = bodies
        .iter()
        .filter(|(_, body)| body.execution_model == ExecutionModel::GpuKernel)
        .map(|(symbol, _)| symbol)
        .collect();
    if kernels.is_empty() {
        return Ok(());
    }

    let reachable: HashSet<&Symbol> = CalleeWalk::from(&body_of, kernels.iter().copied()).collect();
    let mut helpers_to_add = Vec::new();
    for (symbol, body) in bodies.iter() {
        if reachable.contains(symbol)
            && body.execution_model == ExecutionModel::Cpu
            && !kernels.contains(symbol)
        {
            let mut gpu_body = body.clone();
            gpu_body.execution_model = ExecutionModel::GpuDevice;
            copy_out_arguments_in(&mut gpu_body)?;
            answer_count_questions(&mut gpu_body)?;
            narrow_float_types(&mut gpu_body);
            gpu_names.claim_helper(symbol, body.span)?;
            helpers_to_add.push((symbol.clone(), gpu_body));
        }
    }

    // A kernel that calls a (now f32-narrowed) GpuDevice helper must not widen
    // the f32 buffer values it passes to the helper's float params. Narrowing
    // the calling kernel's scalar f64 temps to f32 removes that spurious widen.
    // `narrow_type_kind` only touches scalar floats, so explicit `array<f64>`
    // buffers are left intact; f64-buffer kernels that call no helper are
    // unaffected because they never enter this set.
    let helpers = gpu_names.helpers();
    let kernels_calling_helpers: Vec<usize> = bodies
        .iter()
        .enumerate()
        .filter(|(_, (symbol, body))| {
            body.execution_model == ExecutionModel::GpuKernel
                && CalleeWalk::from(&body_of, [symbol]).any(|callee| helpers.contains(callee))
        })
        .map(|(index, _)| index)
        .collect();
    for index in kernels_calling_helpers {
        narrow_float_types(&mut bodies[index].1);
    }

    bodies.extend(helpers_to_add);
    Ok(())
}

/// Hands every `out` argument of every call in `body` over the way its
/// execution model does ([`out_handover`]). A device helper is cloned from the
/// body lowered for the host, which hands a counted slot, an optional or a
/// value held as its bytes by the place itself; device code copies each into
/// a temp of its own before the call, hands the temp over, and stores it back
/// into the place as the first statement of the block the call continues in.
/// An argument already a temp of its own, as the host copies a word nothing
/// counts into, is left as it is.
///
/// Placement exception: MIR lowering owns how a call hands over an `out`
/// argument, and lowers device code copied in already; this rewrite answers
/// the question a second time because a device helper is cloned from its host
/// body rather than lowered. Lowering every function a kernel reaches as device
/// code would remove it. It buys no speed, so no gain is measured. Guarded by
/// the test `cloning_a_helper_for_a_kernel_copies_its_out_arguments_in`.
pub fn copy_out_arguments_in(body: &mut Body) -> Result<(), LoweringError> {
    for block in 0..body.basic_blocks.len() {
        let restores = copy_call_arguments_in(body, block)?;
        if !restores.is_empty() {
            store_back_after_call(body, block, restores);
        }
    }
    Ok(())
}

/// Copies each `out` argument the call ending `block` hands by its place into
/// a temp of its own, which the call is handed instead; answers the
/// statements storing each temp back into its place.
fn copy_call_arguments_in(body: &mut Body, block: usize) -> Result<Vec<Statement>, LoweringError> {
    let Some(terminator) = body.basic_blocks[block].terminator.as_ref() else {
        return Ok(Vec::new());
    };
    let span = terminator.span;
    let places: Vec<(usize, Place)> = match terminator.passed_arguments() {
        Some((args, modes)) => (args.iter().zip(modes).enumerate())
            .filter_map(|(position, (arg, mode))| {
                handed_place(body, (arg, *mode)).map(|p| (position, p))
            })
            .collect(),
        None => Vec::new(),
    };
    let mut restores = Vec::with_capacity(places.len());
    for (position, place) in places {
        let ty = PlaceTypes::of(body)
            .declared(&place)
            .ok_or_else(|| untyped_out_argument(&place, span))?;
        let representation = Representation::of(&ty.kind, body.layout_scope());
        match out_handover(&representation, body.is_gpu()) {
            OutHandover::ByAddress => continue,
            OutHandover::CopiedIn => {}
        }
        let temp = Place::new(body.new_local(LocalDecl::new(ty, span)));
        let copy = |to: &Place, from: &Place| Statement {
            kind: StatementKind::Assign(to.clone(), Rvalue::Use(Operand::Copy(from.clone()))),
            span,
        };
        body.basic_blocks[block]
            .statements
            .push(copy(&temp, &place));
        restores.push(copy(&place, &temp));
        hand_over(&mut body.basic_blocks[block], position, Operand::Copy(temp));
    }
    Ok(restores)
}

/// The place an `out` argument `arg`, passed as `mode`, hands its callee
/// when its handover is the representation's to say: a field or an element,
/// and in device code a parameter or a declared binding too. A host body
/// hands a whole binding over as its local, which a backend reads back; a
/// temp of its own is the copy already.
fn handed_place(body: &Body, (arg, mode): (&Operand, ArgMode)) -> Option<Place> {
    match mode {
        ArgMode::Out => {}
        ArgMode::Borrowed | ArgMode::Owned | ArgMode::Donated => return None,
    }
    let place = match arg {
        Operand::Copy(place) | Operand::Move(place) => place,
        Operand::Constant(_) | Operand::Function(_) => return None,
    };
    let local = place.local;
    let is_temp = !(1..=body.arg_count).contains(&local.0)
        && !(body.local_decls.get(local.0)).is_some_and(|decl| decl.is_user_variable);
    let is_kept_whole = place.projection.is_empty() && (is_temp || !body.is_gpu());
    (!is_kept_whole).then(|| place.clone())
}

/// Puts `operand` in the argument slot at `position` of the call ending
/// `block`.
fn hand_over(block: &mut BasicBlockData, position: usize, operand: Operand) {
    let Some(terminator) = block.terminator.as_mut() else {
        return;
    };
    let args = match &mut terminator.kind {
        TerminatorKind::Call { args, .. } | TerminatorKind::VirtualCall { args, .. } => args,
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. } => return,
    };
    if let Some(slot) = args.get_mut(position) {
        *slot = operand;
    }
}

/// Puts `restores` first in the block the call ending `block` continues in.
/// A block another edge also enters gets a block of its own in front of it,
/// so the stores run only after this call. A call that never returns stores
/// nothing back.
fn store_back_after_call(body: &mut Body, block: usize, restores: Vec<Statement>) {
    let Some(target) = call_target(body.basic_blocks[block].terminator.as_mut()) else {
        return;
    };
    let Some(continued) = *target else {
        return;
    };
    let entered_once = body.predecessor_counts().get(continued.0) == Some(&1);
    if entered_once {
        let statements = &mut body.basic_blocks[continued.0].statements;
        statements.splice(0..0, restores);
        return;
    }
    let span = restores.first().map_or(body.span, |s| s.span);
    let mut stores = BasicBlockData::new(Some(Terminator::new(
        TerminatorKind::Goto { target: continued },
        span,
    )));
    stores.statements = restores;
    let stores_block = BasicBlock(body.basic_blocks.len());
    if let Some(target) = call_target(body.basic_blocks[block].terminator.as_mut()) {
        *target = Some(stores_block);
    }
    body.basic_blocks.push(stores);
    body.cfg_changed();
}

/// The block a call continues in, as the call records it; `None` for any
/// other terminator.
fn call_target(terminator: Option<&mut Terminator>) -> Option<&mut Option<BasicBlock>> {
    match &mut terminator?.kind {
        TerminatorKind::Call { target, .. } | TerminatorKind::VirtualCall { target, .. } => {
            Some(target)
        }
        TerminatorKind::Goto { .. }
        | TerminatorKind::SwitchInt { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. } => None,
    }
}

/// Answers every question the host body `body` was cloned from asks the
/// runtime about a count: device code holds no counts, so nothing it holds is
/// shared. A check whether a value is shared continues where its answer "no"
/// does, and a copy-on-write call hands its value back as it is. The blocks
/// only a copy path reached are emptied and end in `unreachable`, so device
/// code neither asks nor copies; no block is renumbered. A question in a
/// shape this cannot answer is an internal error, never left for device code
/// to ask.
///
/// Placement exception: MIR lowering owns whether a body asks about a count,
/// and asks nothing in device code; this rewrite strips the questions again
/// because a device helper is cloned from its host body rather than lowered.
/// Lowering every function a kernel reaches as device code would remove it. It
/// buys no speed, so no gain is measured. Guarded by the tests
/// `a_helper_writing_an_element_of_its_own_array_asks_about_no_count` and
/// `a_count_question_device_code_cannot_answer_is_an_internal_error`.
fn answer_count_questions(body: &mut Body) -> Result<(), LoweringError> {
    let unreachable_before: HashSet<usize> = body.find_unreachable_blocks().into_iter().collect();
    let mut answered = false;
    for block in 0..body.basic_blocks.len() {
        let Some(answer) = count_answer(body, block)? else {
            continue;
        };
        let data = &mut body.basic_blocks[block];
        let span = data.terminator.as_ref().map_or(body.span, |old| old.span);
        if let Some(statement) = answer.statement {
            data.statements.push(Statement {
                kind: statement,
                span,
            });
        }
        data.terminator = Some(Terminator::new(
            TerminatorKind::Goto {
                target: answer.target,
            },
            span,
        ));
        answered = true;
    }
    if !answered {
        return Ok(());
    }
    for index in body.find_unreachable_blocks() {
        if !unreachable_before.contains(&index) {
            let data = &mut body.basic_blocks[index];
            let span = data.terminator.as_ref().map_or(body.span, |old| old.span);
            data.statements.clear();
            data.terminator = Some(Terminator::new(TerminatorKind::Unreachable, span));
        }
    }
    body.cfg_changed();
    Ok(())
}

/// What a call about a count is answered with in device code: the statement
/// standing for it, if any, and the block it continues in.
struct CountAnswer {
    statement: Option<StatementKind>,
    target: BasicBlock,
}

/// The answer to the call ending `block` when it asks the runtime about a
/// count ([`crate::runtime_effects::RuntimeEffect`]): a check whether a value
/// is shared, continuing in a block that only branches on its answer, jumps
/// where the answer "no" goes, and a copy-on-write call hands its one
/// argument back. `None` for every other terminator, which the verifier then
/// holds to its rules as it is; an internal error for a check or a
/// copy-on-write call in any other shape, which device code could only ask.
fn count_answer(body: &Body, block: usize) -> Result<Option<CountAnswer>, LoweringError> {
    let Some(terminator) = body.basic_blocks[block].terminator.as_ref() else {
        return Ok(None);
    };
    let TerminatorKind::Call {
        func,
        args,
        destination,
        target: Some(target),
        ..
    } = &terminator.kind
    else {
        return Ok(None);
    };
    let Some(effect) = func.called_runtime_effect() else {
        return Ok(None);
    };
    let unanswered = || unanswered_count_question(func, terminator.span);
    if effect.is_copy_on_write() {
        let [handed] = args.as_slice() else {
            return Err(unanswered());
        };
        return Ok(Some(CountAnswer {
            statement: Some(StatementKind::Assign(
                destination.clone(),
                Rvalue::Use(handed.clone()),
            )),
            target: *target,
        }));
    }
    if !effect.asks_whether_shared() {
        return Ok(None);
    }
    let unshared = branch_on_no(body, *target, destination).ok_or_else(unanswered)?;
    Ok(Some(CountAnswer {
        statement: None,
        target: unshared,
    }))
}

/// The internal error for a call of a device helper's, through `func`,
/// asking the runtime about a count in a shape device code cannot answer.
fn unanswered_count_question(func: &Operand, span: Span) -> LoweringError {
    let name = func
        .called_runtime_name()
        .unwrap_or("a runtime entry point");
    LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        format!(
            "a device helper calls `{name}` about a count in a shape device code cannot answer; \
             device code holds no counts to ask about"
        ),
        span,
    )
}

/// The block `switch` branches to when `answer` is 0, when `switch` does
/// nothing but branch on `answer`, and is entered from one block alone.
///
/// Placement exception: MIR lowering owns where a body goes when a value is
/// not shared; this reads it back off the branch the host lowered because a
/// device helper is cloned from its host body rather than lowered. Lowering
/// every function a kernel reaches as device code would remove it. It buys no
/// speed, so no gain is measured. Guarded by the test
/// `a_count_question_device_code_cannot_answer_is_an_internal_error`.
fn branch_on_no(body: &Body, switch: BasicBlock, answer: &Place) -> Option<BasicBlock> {
    let data = body.basic_blocks.get(switch.0)?;
    let is_entered_once = body.predecessor_counts().get(switch.0) == Some(&1);
    if !data.statements.is_empty() || !is_entered_once {
        return None;
    }
    let TerminatorKind::SwitchInt {
        discr: Operand::Copy(discr) | Operand::Move(discr),
        targets,
        otherwise,
    } = &data.terminator.as_ref()?.kind
    else {
        return None;
    };
    if discr != answer {
        return None;
    }
    let no = Discriminant::from(0);
    let taken = targets.iter().find(|(value, _)| *value == no);
    Some(taken.map_or(*otherwise, |(_, block)| *block))
}

/// The internal error for an `out` argument `place` whose type the body does
/// not record, so no temp can hold its value.
fn untyped_out_argument(place: &Place, span: Span) -> LoweringError {
    LoweringError::internal(
        DiagnosticCode::MirValidationFailed,
        format!("the `out` argument `{place}` of a device helper's call has no type to copy it at"),
        span,
    )
}

/// Narrow all f64 (float) types in the body to f32 for GPU compatibility.
fn narrow_float_types(body: &mut Body) {
    for local_decl in &mut body.local_decls {
        narrow_type_kind(&mut local_decl.ty.kind);
        local_decl.mir_ty = MirType::from_type_kind(&local_decl.ty.kind);
    }
    // Constant operands carry their own type, which drives the WGSL literal
    // width (`2.0lf` for f64). Narrow them too, or an f64 literal survives
    // into an otherwise-f32 helper and trips the SHADER_F64 feature gate.
    for block in &mut body.basic_blocks {
        for stmt in &mut block.statements {
            if let StatementKind::Assign(_, rv) | StatementKind::Reassign(_, rv) = &mut stmt.kind {
                narrow_rvalue_floats(rv);
            }
        }
        if let Some(term) = &mut block.terminator {
            narrow_terminator_floats(&mut term.kind);
        }
    }
}

fn narrow_operand_floats(op: &mut Operand) {
    if let Operand::Constant(c) = op {
        narrow_type_kind(&mut c.ty.kind);
    }
}

fn narrow_rvalue_floats(rv: &mut Rvalue) {
    match rv {
        Rvalue::Use(o) => narrow_operand_floats(o),
        Rvalue::UnaryOp(_, o) => narrow_operand_floats(o),
        Rvalue::BinaryOp(_, a, b) => {
            narrow_operand_floats(a);
            narrow_operand_floats(b);
        }
        Rvalue::Cast(o, ty) => {
            narrow_operand_floats(o);
            narrow_type_kind(&mut ty.kind);
        }
        Rvalue::MathIntrinsic(_, args) | Rvalue::Aggregate(_, args) => {
            for a in args {
                narrow_operand_floats(a);
            }
        }
        Rvalue::Ref(_) | Rvalue::Len(_) | Rvalue::GpuIntrinsic(_) => {}
        Rvalue::AtomicOp {
            value,
            compare_expected,
            ..
        } => {
            narrow_operand_floats(value);
            if let Some(expected) = compare_expected {
                narrow_operand_floats(expected);
            }
        }
    }
}

fn narrow_terminator_floats(term: &mut TerminatorKind) {
    match term {
        TerminatorKind::SwitchInt { discr, .. } => narrow_operand_floats(discr),
        TerminatorKind::Call { func, args, .. } => {
            narrow_operand_floats(func);
            for a in args {
                narrow_operand_floats(a);
            }
        }
        TerminatorKind::VirtualCall { args, .. } => {
            for a in args {
                narrow_operand_floats(a);
            }
        }
        TerminatorKind::Goto { .. }
        | TerminatorKind::Return
        | TerminatorKind::Unreachable
        | TerminatorKind::GpuLaunch { .. } => {}
    }
}

/// Narrow a scalar float type to `f32`; every other type is left as it is.
fn narrow_type_kind(kind: &mut TypeKind) {
    if matches!(kind, TypeKind::Float | TypeKind::F64) {
        *kind = TypeKind::F32;
    }
}
