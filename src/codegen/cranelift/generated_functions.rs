// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The functions the backend generates itself, beside the bodies it is
//! handed: the drop, release and clone thunks of each type, closure
//! destructors and trampolines, and the structural equality and hash of a
//! tuple. None has a source line of its own.
//!
//! A profile build lists each after the user functions, in its function
//! table and in its line map, as a function with no runs, so a profiler
//! tells compiled code with no line apart from the runtime's functions and
//! charges its time to the line that called it.
//!
//! Every one is defined before the first user function is declared, so the
//! module's definable functions at that point that are not user functions
//! are exactly these. A function's code size is known only once its code is
//! emitted, so the function table lists each with no size, and the sizes
//! are filled in from the finished object, before it is written.

use crate::codegen::cranelift::translator::ProfileTable;
use crate::codegen::FunctionCodeMap;
use crate::error::CodegenError;
use crate::mir::Body;
use cranelift_module::{FuncId, Module};
use cranelift_object::object::write::SymbolSection;
use cranelift_object::{ObjectModule, ObjectProduct};
use std::collections::HashSet;

/// The functions the backend generated, by id and link symbol, in the order
/// it declared them.
pub(super) struct GeneratedFunctions {
    functions: Vec<(FuncId, String)>,
}

impl GeneratedFunctions {
    /// Every function `module` declares to define that is not one of
    /// `bodies`.
    pub(super) fn of(module: &ObjectModule, bodies: &[(&str, &Body)]) -> Self {
        let users: HashSet<&str> = bodies.iter().map(|&(name, _)| name).collect();
        let functions = (module.declarations().get_functions())
            .filter(|(_, declaration)| declaration.linkage.is_definable())
            .map(|(id, declaration)| (id, declaration.linkage_name(id).into_owned()))
            .filter(|(_, symbol)| !users.contains(symbol.as_str()))
            .collect();
        Self { functions }
    }

    /// How many there are.
    pub(super) fn len(&self) -> usize {
        self.functions.len()
    }

    /// Each function's entry in the profile function table before its size
    /// is known.
    pub(super) fn unsized_entries(&self) -> impl Iterator<Item = (FuncId, u32)> + '_ {
        self.functions.iter().map(|&(id, _)| (id, 0))
    }

    /// The code size of each function in `product`, in order. A function
    /// declared but never defined is refused: the backend defines every
    /// function it generates.
    pub(super) fn sizes(&self, product: &ObjectProduct) -> Result<Vec<u32>, CodegenError> {
        (self.functions.iter())
            .map(|(id, symbol)| {
                let defined = product.functions.get(*id).copied().flatten();
                let Some((object_symbol, true)) = defined else {
                    return Err(CodegenError::Internal(format!(
                        "the generated function `{symbol}` was never defined"
                    )));
                };
                let size = product.object.symbol(object_symbol).size;
                u32::try_from(size).map_err(|_| {
                    CodegenError::Internal(format!(
                        "the generated function `{symbol}` is too large"
                    ))
                })
            })
            .collect()
    }

    /// Each function's line map entry, of `sizes` bytes each, with no runs.
    pub(super) fn code_maps<'g>(
        &'g self,
        sizes: &'g [u32],
    ) -> impl Iterator<Item = FunctionCodeMap> + 'g {
        (self.functions.iter().zip(sizes)).map(|((_, symbol), &code_size)| FunctionCodeMap {
            symbol: symbol.clone(),
            code_size,
            ranges: Vec::new(),
        })
    }
}

/// Write `sizes` into the profile function table `table` of `product`, from
/// its `first` entry on, each entry `pointer` bytes of address followed by a
/// 32-bit size and padded to two pointers, as the table was defined.
pub(super) fn fill_table_sizes(
    product: &mut ObjectProduct,
    table: ProfileTable,
    first: usize,
    sizes: &[u32],
    pointer: usize,
) -> Result<(), CodegenError> {
    let missing = || CodegenError::Internal("the profile function table was not defined".into());
    let (table_symbol, _) = product
        .data_objects
        .get(table.data)
        .copied()
        .flatten()
        .ok_or_else(missing)?;
    let symbol = product.object.symbol(table_symbol);
    let SymbolSection::Section(section) = symbol.section else {
        return Err(missing());
    };
    let base = usize::try_from(symbol.value).map_err(|_| missing())?;
    let data = product.object.section_mut(section).data_mut();
    for (index, size) in sizes.iter().enumerate() {
        let at = base + (first + index) * 2 * pointer + pointer;
        let slot = data.get_mut(at..at + 4).ok_or_else(missing)?;
        slot.copy_from_slice(&size.to_ne_bytes());
    }
    Ok(())
}
