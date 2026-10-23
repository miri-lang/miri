// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What Cranelift's inliner leaves for the embedder to repair in a caller
//! after copying a callee into it.
//!
//! - A copied `Symbol` global value keeps the callee's reference into the
//!   callee's table of external names, which means nothing in the caller.
//!   Each one is re-declared in the caller's table.
//! - The copied instructions carry no source location. Cranelift creates one
//!   caller block per callee block, numbered from the caller's block count at
//!   the time of the copy in the callee's block order, and fills each copied
//!   block with its original's instructions in layout order, a `return`
//!   becoming a `jump`. When every copied block lines up with its original
//!   opcode for opcode, each copy takes its original's source point,
//!   re-listed in the caller's table as inlined through the call; otherwise
//!   every copy takes the location of the call it replaced.

use super::super::source_points::{SourcePoint, SourcePoints};
use cranelift_codegen::ir::{
    Block, ExternalName, Function, GlobalValue, GlobalValueData, Opcode, SourceLoc,
    UserExternalName,
};

/// A `Symbol` global value of a callee: its index in the callee's table of
/// global values and the name it refers to.
pub(super) struct CopiedSymbol {
    index: u32,
    name: UserExternalName,
}

/// Every `Symbol` global value of `callee` that names a user symbol, or the
/// reason one of them cannot be read.
pub(super) fn copied_symbols(callee: &Function) -> Result<Vec<CopiedSymbol>, String> {
    let mut symbols = Vec::new();
    for (global, data) in callee.global_values.iter() {
        let reference = match data {
            GlobalValueData::Symbol {
                name: ExternalName::User(reference),
                ..
            } => *reference,
            GlobalValueData::Symbol { .. }
            | GlobalValueData::VMContext
            | GlobalValueData::Load { .. }
            | GlobalValueData::IAddImm { .. }
            | GlobalValueData::DynScaleTargetConst { .. } => continue,
        };
        let name = callee
            .params
            .user_named_funcs()
            .get(reference)
            .ok_or_else(|| format!("{global} names no entry of the callee's external names"))?;
        symbols.push(CopiedSymbol {
            index: global.as_u32(),
            name: name.clone(),
        });
    }
    Ok(symbols)
}

/// Point every symbol copied into `caller` from a callee whose global values
/// were appended from `first_global` on at its name in `caller`'s table of
/// external names rather than at the callee's.
pub(super) fn remap_symbols(
    caller: &mut Function,
    symbols: &[CopiedSymbol],
    first_global: u32,
) -> Result<(), String> {
    for symbol in symbols {
        let index = first_global
            .checked_add(symbol.index)
            .ok_or("a copied global value lies past the last index")?;
        let reference = caller.declare_imported_user_function(symbol.name.clone());
        match caller.global_values.get_mut(GlobalValue::from_u32(index)) {
            Some(GlobalValueData::Symbol { name, .. }) => *name = ExternalName::User(reference),
            Some(_) | None => return Err(format!("gv{index} is not the copied symbol")),
        }
    }
    Ok(())
}

/// One instruction of a callee, as the source restore reads it: its opcode
/// and the index of its source point in the callee's table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct CalleeInstruction {
    pub(super) opcode: Opcode,
    pub(super) point: Option<u32>,
}

/// One block of a callee: its number and its instructions in layout order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct CalleeBlock {
    pub(super) index: u32,
    pub(super) instructions: Vec<CalleeInstruction>,
}

/// What restoring a callee's source locations in a caller reads: the
/// callee's blocks, and its source points.
#[derive(Debug, Clone, Default)]
pub(super) struct CalleeSource {
    pub(super) blocks: Vec<CalleeBlock>,
    pub(super) points: Vec<SourcePoint>,
}

impl CalleeSource {
    /// The blocks of `callee` and the points of `points` its locations
    /// index.
    pub(super) fn of(callee: &Function, points: &SourcePoints) -> Self {
        let blocks = (callee.layout.blocks())
            .map(|block| CalleeBlock {
                index: block.as_u32(),
                instructions: (callee.layout.block_insts(block))
                    .map(|inst| {
                        let location = callee.srcloc(inst);
                        CalleeInstruction {
                            opcode: callee.dfg.insts[inst].opcode(),
                            point: points.point_at(location).map(|_| location.bits()),
                        }
                    })
                    .collect(),
            })
            .collect();
        Self {
            blocks,
            points: points.points().to_vec(),
        }
    }

    /// Whether restoring this callee's locations would set none.
    pub(super) fn is_empty(&self) -> bool {
        self.points.is_empty()
    }
}

/// Where one call's copy stands in its caller: the first block Cranelift
/// created for it, and the location of the call it replaced.
#[derive(Debug, Clone, Copy)]
pub(super) struct CopiedCall {
    pub(super) first_block: u32,
    pub(super) call_site: SourceLoc,
}

/// Give each instruction `call` copied into `caller` the source location of
/// the callee instruction it was copied from, listed in `points` as inlined
/// through the call; or, when the copy does not line up with `callee`, the
/// location of the call itself.
pub(super) fn restore_sources(
    caller: &mut Function,
    points: &mut SourcePoints,
    callee: &CalleeSource,
    call: CopiedCall,
) -> Result<(), String> {
    let copies = copied_blocks(caller, callee, call.first_block)?;
    let lined_up = (callee.blocks.iter().zip(&copies)).all(|(original, copy)| {
        copy.as_ref().is_none_or(|copy| {
            original.instructions.len() == copy.len()
                && (original.instructions.iter().zip(copy))
                    .all(|(original, &(_, opcode))| copies_opcode(original.opcode, opcode))
        })
    });
    let call_point = points
        .point_at(call.call_site)
        .map(|_| call.call_site.bits());
    let relisted = lined_up.then(|| relist_points(points, &callee.points, call_point));
    for (original, copy) in callee
        .blocks
        .iter()
        .zip(copies)
        .filter_map(|(o, c)| Some((o, c?)))
    {
        for (position, (inst, _)) in copy.into_iter().enumerate() {
            let restored = relisted.as_ref().and_then(|relisted| {
                let point = original.instructions.get(position)?.point?;
                relisted
                    .get(usize::try_from(point).ok()?)
                    .copied()
                    .flatten()
            });
            let location = restored.map_or(call.call_site, SourceLoc::new);
            if !location.is_default() {
                caller.set_srcloc(inst, location);
            }
        }
    }
    Ok(())
}

/// The instructions of one copied block in layout order, each with its
/// opcode.
type CopiedBlock = Vec<(cranelift_codegen::ir::Inst, Opcode)>;

/// The instructions and opcodes of the caller block copied from each block
/// of `callee`, `None` for a block Cranelift left out as unreachable.
fn copied_blocks(
    caller: &Function,
    callee: &CalleeSource,
    first_block: u32,
) -> Result<Vec<Option<CopiedBlock>>, String> {
    callee
        .blocks
        .iter()
        .map(|original| {
            let index = first_block
                .checked_add(original.index)
                .filter(|&index| usize::try_from(index).is_ok_and(|i| i < caller.dfg.num_blocks()))
                .ok_or("a copied block lies past the caller's last block")?;
            let block = Block::from_u32(index);
            Ok(caller.layout.is_block_inserted(block).then(|| {
                (caller.layout.block_insts(block))
                    .map(|inst| (inst, caller.dfg.insts[inst].opcode()))
                    .collect()
            }))
        })
        .collect()
}

/// List every one of `callee_points` in `points`, a point the callee had
/// not inlined given `call_point` as its call site and every other one the
/// re-listed index of its own; the caller's index of each, by the callee's.
fn relist_points(
    points: &mut SourcePoints,
    callee_points: &[SourcePoint],
    call_point: Option<u32>,
) -> Vec<Option<u32>> {
    let mut relisted: Vec<Option<u32>> = Vec::with_capacity(callee_points.len());
    for point in callee_points {
        let inlined_at = match point.inlined_at {
            Some(index) => usize::try_from(index)
                .ok()
                .and_then(|index| relisted.get(index).copied().flatten()),
            None => call_point,
        };
        let location = points.location_of(SourcePoint {
            inlined_at,
            ..*point
        });
        relisted.push(location.map(|location| location.bits()));
    }
    relisted
}

/// Whether Cranelift's copy of an `original` instruction has the opcode
/// `copy`: the same one, or a jump for a return.
fn copies_opcode(original: Opcode, copy: Opcode) -> bool {
    original == copy || (original == Opcode::Return && copy == Opcode::Jump)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::cranelift::{build_isa, CraneliftOptions};
    use crate::error::SourceId;
    use cranelift_codegen::ir::{types, AbiParam, InstBuilder, Signature};
    use cranelift_codegen::isa::CallConv;
    use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

    /// A caller whose entry block jumps to a second block holding the copy
    /// of a one-block callee: `v + v`, then a jump back out. The entry's
    /// jump carries `call_site`.
    fn caller_with_copy(call_site: SourceLoc) -> Function {
        let mut signature =
            Signature::new(CallConv::triple_default(&target_lexicon::Triple::host()));
        signature.params.push(AbiParam::new(types::I64));
        signature.returns.push(AbiParam::new(types::I64));
        let mut function = Function::with_name_signature(Default::default(), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut function, &mut context);
        let entry = builder.create_block();
        let copy = builder.create_block();
        let exit = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.append_block_param(exit, types::I64);
        builder.switch_to_block(entry);
        let value = builder.block_params(entry)[0];
        builder.set_srcloc(call_site);
        builder.ins().jump(copy, &[]);
        builder.switch_to_block(copy);
        builder.set_srcloc(SourceLoc::default());
        let sum = builder.ins().iadd(value, value);
        builder.ins().jump(exit, &[sum.into()]);
        builder.switch_to_block(exit);
        let result = builder.block_params(exit)[0];
        builder.ins().return_(&[result]);
        builder.seal_all_blocks();
        let isa = build_isa(target_lexicon::Triple::host(), &CraneliftOptions::default())
            .unwrap_or_else(|e| panic!("the host target builds: {e}"));
        builder.finalize(isa.frontend_config());
        function
    }

    fn point(offset: u32, inlined_at: Option<u32>) -> SourcePoint {
        SourcePoint {
            source: SourceId::new(1),
            offset,
            inlined_at,
        }
    }

    /// The callee `caller_with_copy` copied: an add on its point 0 and a
    /// return on its point 1, as `opcodes` says.
    fn callee(opcodes: [Opcode; 2]) -> CalleeSource {
        CalleeSource {
            blocks: vec![CalleeBlock {
                index: 0,
                instructions: vec![
                    CalleeInstruction {
                        opcode: opcodes[0],
                        point: Some(0),
                    },
                    CalleeInstruction {
                        opcode: opcodes[1],
                        point: Some(1),
                    },
                ],
            }],
            points: vec![point(10, None), point(20, None)],
        }
    }

    /// The caller's table, holding the call's own point at index 0.
    fn caller_points() -> (SourcePoints, SourceLoc) {
        let mut points = SourcePoints::default();
        let call = SourcePoint {
            source: SourceId::PROGRAM,
            offset: 5,
            inlined_at: None,
        };
        let location = points.location_of(call).unwrap_or_default();
        (points, location)
    }

    /// The point each instruction of the copied block carries.
    fn copied_points(caller: &Function, points: &SourcePoints) -> Vec<Option<SourcePoint>> {
        let copy = Block::from_u32(1);
        (caller.layout.block_insts(copy))
            .map(|inst| points.point_at(caller.srcloc(inst)))
            .collect()
    }

    #[test]
    fn a_copy_that_lines_up_takes_its_originals_points_inlined_at_the_call() {
        let (mut points, call_site) = caller_points();
        let mut caller = caller_with_copy(call_site);
        let call = CopiedCall {
            first_block: 1,
            call_site,
        };
        restore_sources(
            &mut caller,
            &mut points,
            &callee([Opcode::Iadd, Opcode::Return]),
            call,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            copied_points(&caller, &points),
            vec![Some(point(10, Some(0))), Some(point(20, Some(0)))]
        );
    }

    #[test]
    fn a_copy_that_does_not_line_up_takes_the_calls_location() {
        let (mut points, call_site) = caller_points();
        let mut caller = caller_with_copy(call_site);
        let call = CopiedCall {
            first_block: 1,
            call_site,
        };
        restore_sources(
            &mut caller,
            &mut points,
            &callee([Opcode::Iconst, Opcode::Return]),
            call,
        )
        .unwrap_or_else(|e| panic!("{e}"));
        let call_point = points.point_at(call_site);
        assert_eq!(
            copied_points(&caller, &points),
            vec![call_point, call_point]
        );
    }

    #[test]
    fn a_copy_past_the_callers_blocks_is_refused() {
        let (mut points, call_site) = caller_points();
        let mut caller = caller_with_copy(call_site);
        let call = CopiedCall {
            first_block: 7,
            call_site,
        };
        let callee = callee([Opcode::Iadd, Opcode::Return]);
        assert!(restore_sources(&mut caller, &mut points, &callee, call).is_err());
    }

    #[test]
    fn a_copied_symbol_missing_from_the_caller_is_refused() {
        let mut caller = caller_with_copy(SourceLoc::default());
        let symbol = CopiedSymbol {
            index: 0,
            name: UserExternalName::new(1, 3),
        };
        assert!(remap_symbols(&mut caller, &[symbol], 0).is_err());
    }
}
