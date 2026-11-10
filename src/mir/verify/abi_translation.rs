// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Checks that the element and closure ABI passes changed a body only as the
//! runtime's parameter table says a runtime call is spelled.
//!
//! Every other verifier reads a body before those passes run, when an element
//! or a closure is still one argument and the value it is. This check holds
//! the passes to their word, so what the backend receives is the body those
//! verifiers proved, spelled for the runtime and nothing more:
//!
//! - an element argument becomes the address of the place it was read from
//!   (a constant is first assigned to a temporary) and the count of its
//!   bytes, [`element_layout`]'s payload;
//! - a closure argument is followed by the address of a trampoline;
//! - an element result becomes two trailing arguments, the storage the
//!   runtime writes and its byte count, with the call answering nothing; a
//!   result written to a slot reaches its place through one new block that
//!   copies it there and continues where the call did;
//! - each extra argument is passed borrowed and carries no device handle,
//!   and storage held in a register is passed `out`.
//!
//! Nothing else may change: no local declaration, no statement of the
//! lowered body, no terminator but a runtime call's, and no statement is
//! added but an address or a temporary one of those arguments uses. The
//! positions come from [`param_abi`] and
//! [`crate::runtime_effects::RuntimeEffect::returns_element_value`], the
//! specification the passes read; none of the passes' own code is shared.

use std::collections::{HashMap, HashSet};

use super::{local_display_name, VerificationViolation};
use crate::ast::literal::{IntegerLiteral, Literal};
use crate::ast::types::TypeKind;
use crate::mir::terminator::{ArgMode, TerminatorKind};
use crate::mir::{Body, Local, Operand, Place, Rvalue, StatementKind};
use crate::runtime_effects::RuntimeEffect;
use crate::runtime_fns::{param_abi, RuntimeParamAbi};
use crate::type_checker::element_layout::element_layout;

/// Every way `after` differs from `before` other than the element and closure
/// ABI expansions of its runtime calls.
pub fn verify_abi_translation(before: &Body, after: &Body) -> Vec<VerificationViolation> {
    let mut check = Translation {
        before,
        after,
        added: HashMap::new(),
        copy_blocks: HashSet::new(),
        violations: Vec::new(),
    };
    check.locals();
    for block in 0..before.basic_blocks.len().min(after.basic_blocks.len()) {
        check.block(block);
    }
    check.added_blocks();
    check.violations
}

/// The two bodies, and what the check has found and accounted for so far.
struct Translation<'b> {
    before: &'b Body,
    after: &'b Body,
    /// The value each statement the translation added to the block under
    /// check assigns, by the new local it assigns; removed once an argument
    /// uses it.
    added: HashMap<Local, Rvalue>,
    /// The blocks a result slot is copied out in, each claimed by one call.
    copy_blocks: HashSet<usize>,
    violations: Vec<VerificationViolation>,
}

impl Translation<'_> {
    fn flag(&mut self, local: Local, message: String) {
        self.violations.push(VerificationViolation {
            local,
            local_name: local_display_name(self.after, local),
            message,
        });
    }

    /// Whether `local` is one the translation declared.
    fn is_new(&self, local: Local) -> bool {
        local.0 >= self.before.local_decls.len()
    }

    /// The lowered body's parameters and locals are kept as declared.
    fn locals(&mut self) {
        if self.after.arg_count != self.before.arg_count {
            self.flag(Local(0), "the parameter count changed".to_string());
        }
        if self.after.basic_blocks.len() < self.before.basic_blocks.len() {
            self.flag(
                Local(0),
                "a block of the lowered body is missing".to_string(),
            );
        }
        let kept = self.before.local_decls.len();
        if self.after.local_decls.len() < kept {
            self.flag(
                Local(0),
                "a local of the lowered body is missing".to_string(),
            );
            return;
        }
        for (index, decl) in self.before.local_decls.iter().enumerate() {
            if self.after.local_decls[index] != *decl {
                self.flag(Local(index), "the local's declaration changed".to_string());
            }
        }
    }

    /// One block of the lowered body: its statements kept, followed only by
    /// ones its call's arguments use, and its terminator unchanged unless it
    /// is a runtime call the ABI spells.
    fn block(&mut self, block: usize) {
        self.added.clear();
        self.statements(block);
        let (Some(was), Some(is)) = (
            self.before.basic_blocks[block].terminator.as_ref(),
            self.after.basic_blocks[block].terminator.as_ref(),
        ) else {
            if self.before.basic_blocks[block].terminator
                != self.after.basic_blocks[block].terminator
            {
                self.flag(Local(0), format!("the terminator of bb{block} changed"));
            }
            return;
        };
        match spelled_effect(&was.kind) {
            Some(effect) if was.span == is.span => self.call(block, effect, &was.kind, &is.kind),
            Some(_) | None if was == is => {}
            Some(_) | None => self.flag(Local(0), format!("the terminator of bb{block} changed")),
        }
        let unused: Vec<Local> = self.added.keys().copied().collect();
        for local in unused {
            self.flag(
                local,
                format!("bb{block} has a statement the ABI does not add: nothing reads it"),
            );
        }
    }

    /// The block's statements are the lowered ones, then assignments to new
    /// locals of an address, a constant or a copy of an element.
    fn statements(&mut self, block: usize) {
        let was = &self.before.basic_blocks[block].statements;
        let is = &self.after.basic_blocks[block].statements;
        if is.len() < was.len() || is[..was.len()] != was[..] {
            self.flag(
                Local(0),
                format!("a statement of bb{block} in the lowered body is missing or changed"),
            );
            return;
        }
        for statement in &is[was.len()..] {
            match &statement.kind {
                StatementKind::Assign(place, value)
                    if place.projection.is_empty()
                        && self.is_new(place.local)
                        && is_address_or_element_value(value) =>
                {
                    self.added.insert(place.local, value.clone());
                }
                kind => self.flag(
                    Local(0),
                    format!("bb{block} has a statement the ABI does not add: {kind:?}"),
                ),
            }
        }
    }
}

impl Translation<'_> {
    /// A runtime call the ABI spells: its callee and flags kept, each
    /// argument as [`param_abi`] says it travels, and its result as the
    /// callee hands it back.
    fn call(
        &mut self,
        block: usize,
        effect: &RuntimeEffect,
        was: &TerminatorKind,
        is: &TerminatorKind,
    ) {
        let (
            TerminatorKind::Call {
                func,
                args,
                arg_modes,
                arg_handles,
                flags,
                destination,
                target,
            },
            TerminatorKind::Call {
                func: is_func,
                args: is_args,
                arg_modes: is_modes,
                arg_handles: is_handles,
                flags: is_flags,
                destination: is_destination,
                target: is_target,
            },
        ) = (was, is)
        else {
            self.flag(
                Local(0),
                format!("the call ending bb{block} is no longer a call"),
            );
            return;
        };
        if func != is_func || flags != is_flags {
            self.flag(
                Local(0),
                format!("the callee or flags of the call ending bb{block} changed"),
            );
            return;
        }
        let mut spelled = Spelled::default();
        for (position, arg) in args.iter().enumerate() {
            let words = self.argument(
                effect,
                position,
                arg,
                &is_args[spelled.args.min(is_args.len())..],
            );
            spelled.widen(words, position, arg_modes, arg_handles);
        }
        let result = CallResult {
            destination,
            target,
            is_destination,
            is_target,
        };
        if effect.returns_element_value() {
            self.result(
                block,
                &is_args[spelled.args.min(is_args.len())..],
                &result,
                &mut spelled,
            );
        } else if destination != is_destination || target != is_target {
            self.flag(
                destination.local,
                format!("the result of the call ending bb{block} changed"),
            );
        }
        if spelled.args != is_args.len() {
            let count = format!(
                "{} arguments, not the {} the ABI spells",
                is_args.len(),
                spelled.args
            );
            self.flag(Local(0), format!("the call ending bb{block} takes {count}"));
        }
        if *is_modes != spelled.modes || *is_handles != spelled.handles {
            self.flag(Local(0), format!("the argument modes or handles of the call ending bb{block} are not the ones the ABI widens to"));
        }
    }

    /// Check the argument at declared `position`, `arg`, against the
    /// spelled arguments from `rest` on, answering how many it travels as.
    fn argument(
        &mut self,
        effect: &RuntimeEffect,
        position: usize,
        arg: &Operand,
        rest: &[Operand],
    ) -> usize {
        let abi = param_abi(effect, position);
        let words = abi.words();
        if rest.len() < words {
            self.flag(
                Local(0),
                format!("argument {position} is missing from the spelled call"),
            );
            return words;
        }
        match abi {
            RuntimeParamAbi::Word if rest[0] == *arg => {}
            RuntimeParamAbi::Word => self.flag(
                Local(0),
                format!("argument {position} changed though it travels as itself"),
            ),
            RuntimeParamAbi::Element => {
                let stored = effect.stores(position);
                self.element(position, stored, arg, &rest[0], &rest[1])
            }
            RuntimeParamAbi::Closure(_) => self.closure(position, arg, &rest[0], &rest[1]),
        }
        words
    }

    /// An element argument spelled as the address of the place it was read
    /// from, or, for an element the container stores, of a temporary the pass
    /// filled with a copy of it, then its byte count.
    fn element(
        &mut self,
        position: usize,
        stored: bool,
        arg: &Operand,
        address: &Operand,
        count: &Operand,
    ) {
        let Some(addressed) = self.address_of(address) else {
            self.flag(
                Local(0),
                format!("argument {position} is not the address of an element"),
            );
            return;
        };
        let read = match arg {
            Operand::Copy(place) | Operand::Move(place) if stored => {
                let copied = Operand::Copy(place.clone());
                (self.temporary_of(&addressed, &copied)).unwrap_or_else(|| place.clone())
            }
            Operand::Copy(place) | Operand::Move(place) => place.clone(),
            Operand::Constant(_) => match self.temporary_of(&addressed, arg) {
                Some(temporary) => temporary,
                None => {
                    self.flag(addressed.local, format!("argument {position} takes the address of {addressed}, not of a temporary holding the constant"));
                    return;
                }
            },
            Operand::Function(_) => {
                self.flag(
                    Local(0),
                    format!("argument {position} hands a function as an element"),
                );
                return;
            }
        };
        if addressed != read {
            self.flag(addressed.local, format!("argument {position} takes the address of {addressed}, not of the element {read}"));
            return;
        }
        let kind = match arg {
            Operand::Constant(constant) => Some(constant.ty.kind.clone()),
            Operand::Copy(_) | Operand::Move(_) | Operand::Function(_) => self.whole_kind(&read),
        };
        self.byte_count(position, kind.as_ref(), count);
    }

    /// A closure argument kept, followed by the address of a trampoline.
    fn closure(&mut self, position: usize, arg: &Operand, kept: &Operand, trampoline: &Operand) {
        if kept != arg {
            self.flag(Local(0), format!("closure argument {position} changed"));
        }
        let is_trampoline = matches!(trampoline, Operand::Function(function)
            if function.symbol.trampoline_shape().is_some());
        if !is_trampoline {
            self.flag(
                Local(0),
                format!("closure argument {position} is not followed by a trampoline"),
            );
        }
    }
}

impl Translation<'_> {
    /// The element result of a call, spelled as trailing `storage` and byte
    /// count, the call answering a new local that holds nothing.
    fn result(
        &mut self,
        block: usize,
        trailing: &[Operand],
        result: &CallResult,
        spelled: &mut Spelled,
    ) {
        let [storage, count] = trailing else {
            self.flag(result.destination.local, format!("the call ending bb{block} hands its element result back other than through storage and a byte count"));
            return;
        };
        spelled.args += 2;
        let nothing = &result.is_destination;
        let holds_nothing = nothing.projection.is_empty()
            && self.is_new(nothing.local)
            && self.after.local_decls[nothing.local.0].ty.kind == TypeKind::Void;
        if !holds_nothing {
            self.flag(
                nothing.local,
                format!("the call ending bb{block} still answers a value"),
            );
        }
        let (written, is_out) = match self.address_of(storage) {
            Some(addressed) => (addressed, false),
            None => match storage {
                Operand::Copy(place) => (place.clone(), true),
                Operand::Move(_) | Operand::Constant(_) | Operand::Function(_) => {
                    self.flag(
                        Local(0),
                        format!("the result storage of the call ending bb{block} is not a place"),
                    );
                    return;
                }
            },
        };
        if is_out {
            let modes = spelled.args - 2;
            spelled.modes.resize(modes, ArgMode::Borrowed);
            spelled.modes.extend([ArgMode::Out, ArgMode::Borrowed]);
        }
        self.reaches_destination(block, &written, result);
        let kind = self.whole_kind(&written);
        self.byte_count(spelled.args - 1, kind.as_ref(), count);
    }

    /// The runtime writes `written`, which is the call's destination, or a
    /// new slot one new block copies into the destination before continuing
    /// where the call did.
    fn reaches_destination(&mut self, block: usize, written: &Place, result: &CallResult) {
        if written == result.destination {
            if result.target != result.is_target {
                self.flag(
                    written.local,
                    format!("the call ending bb{block} continues elsewhere"),
                );
            }
            return;
        }
        if !(written.projection.is_empty() && self.is_new(written.local)) {
            self.flag(written.local, format!("the call ending bb{block} writes its element to {written}, neither its destination nor a new slot"));
            return;
        }
        let Some(continuation) = *result.target else {
            return;
        };
        let copied = (*result.is_target).and_then(|copy| {
            let data = self.after.basic_blocks.get(copy.0)?;
            let copies = data.statements.len() == 1
                && matches!(&data.statements[0].kind, StatementKind::Assign(place, Rvalue::Use(Operand::Copy(slot)))
                    if place == result.destination && slot == written);
            let continues = matches!(data.terminator.as_ref().map(|t| &t.kind),
                Some(TerminatorKind::Goto { target }) if *target == continuation);
            (copy.0 >= self.before.basic_blocks.len() && copies && continues).then_some(copy.0)
        });
        match copied {
            Some(copy) if self.copy_blocks.insert(copy) => {}
            Some(_) | None => self.flag(written.local, format!("the element the call ending bb{block} writes to {written} is not copied into {} before the call continues", result.destination)),
        }
    }

    /// Every block the translation added is the copy-out block of one call.
    fn added_blocks(&mut self) {
        for block in self.before.basic_blocks.len()..self.after.basic_blocks.len() {
            if !self.copy_blocks.contains(&block) {
                self.flag(
                    Local(0),
                    format!("bb{block} is a block the ABI does not add"),
                );
            }
        }
    }
}

impl Translation<'_> {
    /// The place an argument's address was taken of, when the argument is a
    /// new local the block assigns `&place`; that assignment is accounted for.
    fn address_of(&mut self, address: &Operand) -> Option<Place> {
        let Operand::Copy(local) = address else {
            return None;
        };
        if !local.projection.is_empty() {
            return None;
        }
        let Some(Rvalue::Ref(place)) = self.added.get(&local.local) else {
            return None;
        };
        let place = place.clone();
        self.added.remove(&local.local);
        Some(place)
    }

    /// `addressed`, when it is a new local the block assigns the constant
    /// `arg`; that assignment is accounted for.
    fn temporary_of(&mut self, addressed: &Place, arg: &Operand) -> Option<Place> {
        if !addressed.projection.is_empty() {
            return None;
        }
        let Some(Rvalue::Use(value)) = self.added.get(&addressed.local) else {
            return None;
        };
        if value != arg {
            return None;
        }
        self.added.remove(&addressed.local);
        Some(addressed.clone())
    }

    /// The declared type of `place` when it is a whole local; a projected
    /// place is sized by what its projection reaches, which only the pass
    /// spelling it reads.
    fn whole_kind(&self, place: &Place) -> Option<TypeKind> {
        place
            .projection
            .is_empty()
            .then(|| self.after.local_decls.get(place.local.0))
            .flatten()
            .map(|decl| decl.ty.kind.clone())
    }

    /// `count` is a pointer-sized constant: the payload of an element of
    /// `kind` where the type is known, and some bytes where it is not.
    fn byte_count(&mut self, position: usize, kind: Option<&TypeKind>, count: &Operand) {
        let bytes = match count {
            Operand::Constant(constant) => match constant.literal {
                Literal::Integer(IntegerLiteral::I64(bytes)) => Some(bytes),
                _ => None,
            },
            Operand::Copy(_) | Operand::Move(_) | Operand::Function(_) => None,
        };
        let expected = kind.map(|kind| element_layout(kind, self.after.layout_scope()).payload);
        let right = match (bytes, expected) {
            (Some(bytes), Some(expected)) => bytes == expected,
            (Some(bytes), None) => bytes > 0,
            (None, _) => false,
        };
        if !right {
            self.flag(
                Local(0),
                format!("argument {position} carries the byte count {count}, not {expected:?}"),
            );
        }
    }
}

/// What a spelled call's arguments and their metadata should be, built
/// alongside the check from the lowered call's.
#[derive(Default)]
struct Spelled {
    /// How many spelled arguments the lowered ones account for.
    args: usize,
    modes: Vec<ArgMode>,
    handles: Vec<Option<crate::mir::body::DeviceHandleId>>,
}

impl Spelled {
    /// Account for the lowered argument at `position`, which travels as
    /// `words` arguments: each one past the first is passed borrowed and
    /// carries no handle, where the lowered call recorded either.
    fn widen(
        &mut self,
        words: usize,
        position: usize,
        modes: &[ArgMode],
        handles: &[Option<crate::mir::body::DeviceHandleId>],
    ) {
        self.args += words;
        if let Some(mode) = modes.get(position) {
            self.modes.push(*mode);
            self.modes.extend((1..words).map(|_| ArgMode::Borrowed));
        }
        if let Some(handle) = handles.get(position) {
            self.handles.push(*handle);
            self.handles.extend((1..words).map(|_| None));
        }
    }
}

/// A call's destination and successor, as lowered and as spelled.
struct CallResult<'c> {
    destination: &'c crate::mir::Place,
    target: &'c Option<crate::mir::BasicBlock>,
    is_destination: &'c crate::mir::Place,
    is_target: &'c Option<crate::mir::BasicBlock>,
}

/// The record of the runtime entry point `kind` calls, when the ABI spells
/// any of its arguments or its result.
fn spelled_effect(kind: &TerminatorKind) -> Option<&RuntimeEffect> {
    let TerminatorKind::Call { func, .. } = kind else {
        return None;
    };
    let effect = func.called_runtime_effect()?;
    let spells = effect.returns_element_value()
        || !effect.element_positions().is_empty()
        || !effect.closure_positions().is_empty();
    spells.then_some(effect)
}

/// Whether `value` is what the translation assigns a new local: an address,
/// or a constant or a copy of an element an address is then taken of. A copy
/// is accounted for only where the call hands a stored element
/// ([`Translation::element`]); anywhere else nothing reads it, and it is
/// flagged.
fn is_address_or_element_value(value: &Rvalue) -> bool {
    matches!(
        value,
        Rvalue::Ref(_) | Rvalue::Use(Operand::Constant(_) | Operand::Copy(_))
    )
}
