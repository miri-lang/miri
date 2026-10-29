// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The closure ABI at the runtime boundary: how a runtime function is handed
//! a closure it calls.
//!
//! A call into the runtime is lowered with a closure as one argument, which is
//! how reference counting and verification read it: a value the call borrows.
//! The runtime entry point takes that argument as two words — the closure,
//! then the address of a trampoline the compiler generates for the closure's
//! parameter types. Which parameters carry a closure, and what the runtime
//! promises about calling it, is [`crate::runtime_fns::closure_contract`]; the
//! type checker holds every `runtime` declaration to that contract, and this
//! pass holds every call to it again, against the types the call is made at.
//! The runtime calls the trampoline with the closure, the address of each
//! argument's bytes and the address its result goes to; the trampoline loads
//! what the closure takes out of each address, calls it and stores what it
//! answers.
//!
//! The runtime hands the trampoline the addresses of collection element
//! slots, so a trampoline argument reads a slot exactly as the collection
//! holds an element of that type (see [`element_layout`]). Where the slot and
//! the way the closure takes the value disagree, the pass reports it rather
//! than guessing at a conversion.
//!
//! This pass spells the second word in MIR as the trampoline's address, a
//! function operand, so no backend has to know which runtime parameters are
//! closures. It runs after reference counting, which has already accounted
//! for the closure as the value it is, and after the element ABI is spelled.
//! A GPU body calls no runtime function and is left alone.

use crate::ast::common::Parameter;
use crate::ast::factory::{make_type, type_expr_non_null};
use crate::ast::types::{atomic_inner_kind, FunctionTypeData, Type, TypeKind, VALUE_WORD_BYTES};
use crate::error::syntax::Span;
use crate::mir::element_abi::element_layout;
use crate::mir::instantiation::type_argument;
use crate::mir::place_type::PlaceTypes;
use crate::mir::symbol::{Symbol, TrampolineArgument, TrampolineShape, TrampolineWord};
use crate::mir::{ArgMode, Body, FunctionConstant, Operand, TerminatorKind};
use crate::runtime_fns::{ClosureContract, RuntimeParamAbi, RuntimeParamTable, RuntimeSurface};

/// Follow every closure argument of a runtime call in `body` with the address
/// of the trampoline the runtime calls it through.
///
/// A call that breaks the closure's contract — a closure of another arity or
/// result, parameters a collection element slot cannot be read as, a value
/// that is not a closure — is reported.
pub fn pass_closures_with_trampolines(body: &mut Body) -> Result<(), String> {
    pass_closures_through(body, &RuntimeSurface)
}

/// [`pass_closures_with_trampolines`] against the runtime parameters `table`
/// describes.
pub fn pass_closures_through(body: &mut Body, table: &dyn RuntimeParamTable) -> Result<(), String> {
    if body.is_gpu() {
        return Ok(());
    }
    for block in 0..body.basic_blocks.len() {
        let trampolines = closure_trampolines(body, block, table)?;
        insert_trampolines(body, block, trampolines);
    }
    Ok(())
}

/// Each closure argument of the runtime call ending `block`, by its index
/// among the call's arguments, with the trampoline operand that follows it,
/// in the order of the arguments.
fn closure_trampolines(
    body: &Body,
    block: usize,
    table: &dyn RuntimeParamTable,
) -> Result<Vec<(usize, Operand)>, String> {
    let Some(terminator) = body.basic_blocks[block].terminator.as_ref() else {
        return Ok(Vec::new());
    };
    let TerminatorKind::Call { func, args, .. } = &terminator.kind else {
        return Ok(Vec::new());
    };
    let Some(name) = func.called_runtime_name() else {
        return Ok(Vec::new());
    };
    let mut trampolines = Vec::new();
    for &position in table.closure_positions(name) {
        let call = ClosureCall {
            body,
            table,
            name,
            args,
            position,
        };
        let shape = call.checked_shape()?;
        let index = table.argument_index(name, position);
        trampolines.push((index, trampoline_operand(shape, terminator.span)));
    }
    Ok(trampolines)
}

/// One closure argument of one runtime call.
struct ClosureCall<'b> {
    body: &'b Body,
    table: &'b dyn RuntimeParamTable,
    name: &'b str,
    args: &'b [Operand],
    position: usize,
}

impl ClosureCall<'_> {
    /// The shape of the trampoline the closure is called through, after
    /// checking it against the runtime's contract for the parameter.
    fn checked_shape(&self) -> Result<TrampolineShape, String> {
        let RuntimeParamAbi::Closure(contract) = self.table.param_abi(self.name, self.position)
        else {
            return Err(self.refusal("names a closure parameter the runtime has no contract for"));
        };
        let function = self
            .argument(self.position)
            .and_then(|arg| closure_type(self.body, arg).transpose())
            .transpose()?
            .ok_or_else(|| self.refusal("is not handed a closure"))?;
        let shape = trampoline_shape(self.body, &function)?;
        self.check_contract(&shape, contract)?;
        Ok(shape)
    }

    /// Refuse a shape the runtime does not call: another arity, a result that
    /// is not one pointer-sized word, or an argument read otherwise than the
    /// collection the arguments come from holds its elements.
    fn check_contract(
        &self,
        shape: &TrampolineShape,
        contract: ClosureContract,
    ) -> Result<(), String> {
        if shape.arguments.len() != contract.arity {
            return Err(self.refusal(&format!(
                "is handed a closure taking {} arguments, where the runtime passes {}",
                shape.arguments.len(),
                contract.arity
            )));
        }
        if shape.result != Some(TrampolineWord::Pointer) {
            return Err(self.refusal("is handed a closure that does not answer an `int`"));
        }
        let Some(collection) = contract.elements_of else {
            return Ok(());
        };
        let element = self.element_argument(collection)?;
        match shape
            .arguments
            .iter()
            .find(|&&argument| argument != element)
        {
            Some(_) => Err(self.refusal(
                "is handed a closure whose parameters are not read as the collection holds \
                 its elements",
            )),
            None => Ok(()),
        }
    }

    /// How a trampoline reads an element of the collection argument
    /// `collection` out of its slot.
    fn element_argument(&self, collection: usize) -> Result<TrampolineArgument, String> {
        let element = self
            .argument(collection)
            .and_then(|arg| operand_type(self.body, arg))
            .and_then(|ty| ty.kind.sequence_element_kind().cloned())
            .ok_or_else(|| self.refusal("is called over a value that is not a sequence"))?;
        trampoline_argument(self.body, &element)
    }

    /// The call argument the declared parameter `position` is passed as, in
    /// the call as this pass finds it: every element already two words, and
    /// no closure yet followed by its trampoline.
    fn argument(&self, position: usize) -> Option<&Operand> {
        let trampolines_before = self
            .table
            .closure_positions(self.name)
            .iter()
            .filter(|&&closure| closure < position)
            .count();
        let index = self.table.argument_index(self.name, position) - trampolines_before;
        self.args.get(index)
    }

    fn refusal(&self, what: &str) -> String {
        format!(
            "parameter {} of the runtime function `{}` {what}",
            self.position, self.name
        )
    }
}

/// The type the operand `arg` carries, as the body declares it.
fn operand_type(body: &Body, arg: &Operand) -> Option<Type> {
    match arg {
        Operand::Copy(place) | Operand::Move(place) if place.projection.is_empty() => {
            Some(body.local_decls[place.local.0].ty.clone())
        }
        Operand::Copy(place) | Operand::Move(place) => PlaceTypes::of(body).declared(place),
        Operand::Constant(constant) => Some(constant.ty.clone()),
        Operand::Function(function) => Some(function.ty.clone()),
    }
}

/// The function type of the closure `arg` carries, or `None` when it carries
/// no closure. A function named directly is a code address, not a closure,
/// and is refused.
fn closure_type(body: &Body, arg: &Operand) -> Result<Option<FunctionTypeData>, String> {
    if let Operand::Function(function) = arg {
        return Err(format!(
            "the function {} is handed to the runtime as a closure, which it is not",
            function.symbol
        ));
    }
    Ok(operand_type(body, arg).and_then(|ty| {
        if let TypeKind::Function(data) = ty.kind {
            Some(*data)
        } else {
            None
        }
    }))
}

/// How the trampoline for a closure of type `function` hands each argument on
/// and stores the result.
fn trampoline_shape(body: &Body, function: &FunctionTypeData) -> Result<TrampolineShape, String> {
    let arguments = function
        .params
        .iter()
        .map(|param| trampoline_argument(body, &written_kind(&param.typ)?))
        .collect::<Result<_, _>>()?;
    let result = match function.return_type.as_deref() {
        Some(ret) => trampoline_result(body, &written_kind(ret)?)?,
        None => None,
    };
    Ok(TrampolineShape { arguments, result })
}

/// The type a closure parameter or result is written as.
fn written_kind(expr: &crate::ast::expression::Expression) -> Result<TypeKind, String> {
    type_argument(expr)
        .map(|ty| ty.kind)
        .ok_or_else(|| "a closure handed to the runtime names a type this pass cannot read".into())
}

/// How a trampoline reads an argument of type `kind` out of the element slot
/// whose address it is handed, as the closure takes it.
///
/// A value held as its bytes is held inline in the slot and taken by address.
/// Any other value is one word, which must fill the slot exactly.
fn trampoline_argument(body: &Body, kind: &TypeKind) -> Result<TrampolineArgument, String> {
    if body.value_layout(kind).is_some() {
        return Ok(TrampolineArgument::Address);
    }
    let slot = element_layout(kind, body.layout_scope());
    let word = register_word(kind);
    if word.bytes(VALUE_WORD_BYTES) != slot.payload {
        return Err(format!(
            "a closure handed to the runtime takes a `{kind}` as a {}-byte word, but a \
             collection holds one in {} bytes",
            word.bytes(VALUE_WORD_BYTES),
            slot.payload
        ));
    }
    Ok(TrampolineArgument::Word(word))
}

/// The word a trampoline stores a result of type `kind` as, or `None` when
/// the closure answers nothing. A result held as its bytes comes back through
/// a slot of the caller's, which a trampoline does not provide.
fn trampoline_result(body: &Body, kind: &TypeKind) -> Result<Option<TrampolineWord>, String> {
    if matches!(kind, TypeKind::Void) {
        return Ok(None);
    }
    if body.value_layout(kind).is_some() {
        return Err(format!(
            "a closure handed to the runtime answers a `{kind}`, a value held as its bytes, \
             which a trampoline cannot store"
        ));
    }
    Ok(Some(register_word(kind)))
}

/// The word a value of type `kind` travels in when it is not held as its
/// bytes: the width a backend gives the scalar, or a pointer.
fn register_word(kind: &TypeKind) -> TrampolineWord {
    if let Some(inner) = atomic_inner_kind(kind) {
        return register_word(inner);
    }
    match kind {
        TypeKind::I8 | TypeKind::U8 | TypeKind::Boolean | TypeKind::Void => TrampolineWord::I8,
        TypeKind::I16 | TypeKind::U16 => TrampolineWord::I16,
        TypeKind::I32 | TypeKind::U32 => TrampolineWord::I32,
        TypeKind::I64 | TypeKind::U64 => TrampolineWord::I64,
        TypeKind::I128 | TypeKind::U128 => TrampolineWord::I128,
        TypeKind::F16 | TypeKind::F32 => TrampolineWord::F32,
        TypeKind::F64 | TypeKind::Float => TrampolineWord::F64,
        TypeKind::Int
        | TypeKind::Linear(_)
        | TypeKind::String
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Set(_)
        | TypeKind::Tuple(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Custom(_, _)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Error
        | TypeKind::OneOf(_) => TrampolineWord::Pointer,
    }
}

/// The address of the trampoline of `shape`, typed as the function it is: the
/// closure, an address per argument and the result's address, answering
/// nothing.
fn trampoline_operand(shape: TrampolineShape, span: Span) -> Operand {
    let address = || Parameter {
        name: String::new(),
        name_span: Span::default(),
        typ: Box::new(type_expr_non_null(Type::new(TypeKind::RawPtr, span))),
        guard: None,
        default_value: None,
        is_out: false,
        residency: None,
    };
    let params = (0..shape.arguments.len() + 2).map(|_| address()).collect();
    let ty = make_type(TypeKind::Function(Box::new(FunctionTypeData {
        generics: None,
        params,
        return_type: None,
    })));
    Operand::Function(Box::new(FunctionConstant {
        span,
        ty,
        symbol: Symbol::closure_trampoline(shape),
    }))
}

/// Insert each trampoline right after the closure it belongs to, in the call
/// ending `block`, widening the per-argument metadata to match: a trampoline
/// is passed borrowed and carries no device handle. Each index already counts
/// the trampolines of the closures before it, so they go in in order.
fn insert_trampolines(body: &mut Body, block: usize, trampolines: Vec<(usize, Operand)>) {
    let Some(terminator) = body.basic_blocks[block].terminator.as_mut() else {
        return;
    };
    let TerminatorKind::Call {
        args,
        arg_modes,
        arg_handles,
        ..
    } = &mut terminator.kind
    else {
        return;
    };
    for (position, trampoline) in trampolines {
        args.insert(position + 1, trampoline);
        if position < arg_modes.len() {
            arg_modes.insert(position + 1, ArgMode::Borrowed);
        }
        if position < arg_handles.len() {
            arg_handles.insert(position + 1, None);
        }
    }
}
