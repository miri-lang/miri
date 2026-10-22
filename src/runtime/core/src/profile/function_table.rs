// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The compiled functions of a profile build, found by the address of an
//! instruction inside them.

/// One function of the table a profile build hands the runtime: where its
/// code was loaded and how many bytes of it there are. The layout is the one
/// the compiler writes for `miri_profile_functions`: a pointer-sized address
/// then a 32-bit size, padded to two pointers.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProfileFunction {
    pub address: usize,
    pub code_size: u32,
}

/// Where an address falls: the function's index in the table the build
/// wrote, which is its index in the build's line map, and the byte offset
/// from the function's first instruction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Location {
    pub index: u32,
    pub offset: u32,
}

/// One function's code range, kept with its index in the table it came from.
#[derive(Debug, Clone, Copy)]
struct CodeRange {
    start: usize,
    end: usize,
    index: u32,
}

/// The table's functions sorted by load address, so an address is placed by
/// binary search. Addresses are read after the program was loaded, so the
/// table holds wherever ASLR put the code and needs no symbol table.
#[derive(Debug, Default)]
pub(crate) struct FunctionTable {
    ranges: Vec<CodeRange>,
}

impl FunctionTable {
    /// The table of `functions`, a function with no code left out.
    pub(crate) fn new(functions: &[ProfileFunction]) -> Self {
        let mut ranges: Vec<CodeRange> = (0u32..)
            .zip(functions)
            .filter(|(_, function)| function.code_size > 0)
            .map(|(index, function)| CodeRange {
                start: function.address,
                end: function.address.saturating_add(function.code_size as usize),
                index,
            })
            .collect();
        ranges.sort_unstable_by_key(|range| range.start);
        Self { ranges }
    }

    /// The first address and the code size of the function `address` falls
    /// inside, if any. Reads only the table, which is not written after it is
    /// built, so a signal handler may ask.
    pub(crate) fn code_of(&self, address: usize) -> Option<(usize, usize)> {
        let range = self.range_of(address)?;
        Some((range.start, range.end - range.start))
    }

    /// The range of the function `address` falls inside, if any.
    fn range_of(&self, address: usize) -> Option<&CodeRange> {
        let after = self.ranges.partition_point(|range| range.start <= address);
        let range = self.ranges.get(after.checked_sub(1)?)?;
        (address < range.end).then_some(range)
    }

    /// The function `address` falls inside, if any.
    pub(crate) fn locate(&self, address: usize) -> Option<Location> {
        let range = self.range_of(address)?;
        Some(Location {
            index: range.index,
            offset: (address - range.start) as u32,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn function(address: usize, code_size: u32) -> ProfileFunction {
        ProfileFunction { address, code_size }
    }

    #[test]
    fn an_address_is_placed_in_the_function_holding_it_whatever_the_table_order() {
        let table = FunctionTable::new(&[
            function(0x3000, 0x100),
            function(0x1000, 0x40),
            function(0x2000, 0x10),
        ]);
        assert_eq!(
            table.locate(0x1000),
            Some(Location {
                index: 1,
                offset: 0
            })
        );
        assert_eq!(
            table.locate(0x103f),
            Some(Location {
                index: 1,
                offset: 0x3f
            })
        );
        assert_eq!(
            table.locate(0x2008),
            Some(Location {
                index: 2,
                offset: 8
            })
        );
        assert_eq!(
            table.locate(0x30ff),
            Some(Location {
                index: 0,
                offset: 0xff
            })
        );
    }

    #[test]
    fn an_address_between_or_outside_the_functions_is_in_none() {
        let table = FunctionTable::new(&[function(0x1000, 0x40), function(0x2000, 0x10)]);
        assert_eq!(table.locate(0xfff), None);
        assert_eq!(table.locate(0x1040), None);
        assert_eq!(table.locate(0x2010), None);
        assert_eq!(table.locate(usize::MAX), None);
        assert_eq!(FunctionTable::new(&[]).locate(0x1000), None);
    }

    #[test]
    fn the_code_of_an_address_is_its_whole_function() {
        let table = FunctionTable::new(&[function(0x1000, 0x40), function(0x2000, 0x10)]);
        assert_eq!(table.code_of(0x2008), Some((0x2000, 0x10)));
        assert_eq!(table.code_of(0x1040), None);
    }

    #[test]
    fn a_function_with_no_code_holds_no_address() {
        let table = FunctionTable::new(&[function(0x1000, 0), function(0x1000, 8)]);
        assert_eq!(
            table.locate(0x1000),
            Some(Location {
                index: 1,
                offset: 0
            })
        );
    }

    #[test]
    fn the_layout_matches_the_table_the_compiler_writes() {
        let pointer = std::mem::size_of::<usize>();
        assert_eq!(std::mem::size_of::<ProfileFunction>(), 2 * pointer);
        assert_eq!(std::mem::offset_of!(ProfileFunction, code_size), pointer);
    }
}
