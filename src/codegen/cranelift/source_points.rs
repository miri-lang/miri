// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The places in the program a function's instructions were generated from.
//!
//! An instruction's `SourceLoc` is not a byte offset but an index into its
//! function's [`SourcePoints`], and each point names its file as well as its
//! offset. A function holds code of more than one file once a callee from
//! another module is inlined into it, and a bare offset could not say which
//! file it indexes. A point of inlined code also names the point of the call
//! it was inlined through, so a chain of points leads from code inlined
//! through nested calls out to a line of the function itself.

use crate::codegen::SourcePosition;
use crate::error::SourceId;
use cranelift_codegen::ir::SourceLoc;
use std::collections::HashMap;

/// One place in the program's source: a file and a byte offset into it,
/// and, for inlined code, the index of the point of the call it was inlined
/// through, in the same table.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SourcePoint {
    pub(crate) source: SourceId,
    pub(crate) offset: u32,
    pub(crate) inlined_at: Option<u32>,
}

/// Every place one function's instructions were generated from, each listed
/// once, in the order the function first reached it. A point's call site is
/// always listed before it, so following call sites always ends.
#[derive(Debug, Clone, Default)]
pub(crate) struct SourcePoints {
    points: Vec<SourcePoint>,
    locations: HashMap<SourcePoint, u32>,
    /// Where the function itself is written, the place its code before its
    /// first statement is charged to; `None` for a function synthesized
    /// with no source.
    function: Option<SourcePoint>,
}

impl SourcePoints {
    /// The location instructions generated from `point` carry, `point` listed
    /// first if it is new. `None` once the table is too long for a location
    /// to index it, and for a point naming a call site not yet listed.
    pub(crate) fn location_of(&mut self, point: SourcePoint) -> Option<SourceLoc> {
        if let Some(&index) = self.locations.get(&point) {
            return Some(SourceLoc::new(index));
        }
        let index = self.points.len();
        if point
            .inlined_at
            .is_some_and(|call| usize::try_from(call).map_or(true, |call| call >= index))
        {
            return None;
        }
        let location = location_for(index)?;
        self.points.push(point);
        self.locations.insert(point, location.bits());
        Some(location)
    }

    /// The location of `point`, listed as where the function itself is
    /// written; `None`, and nothing listed, as [`Self::location_of`] says.
    pub(crate) fn locate_function(&mut self, point: SourcePoint) -> Option<SourceLoc> {
        let location = self.location_of(point)?;
        self.function = Some(point);
        Some(location)
    }

    /// Where the function itself is written, when it was listed.
    pub(crate) fn function_point(&self) -> Option<SourcePoint> {
        self.function
    }

    /// The point `location` indexes, `None` for the default location and
    /// for one this table did not hand out.
    pub(crate) fn point_at(&self, location: SourceLoc) -> Option<SourcePoint> {
        if location.is_default() {
            return None;
        }
        usize::try_from(location.bits())
            .ok()
            .and_then(|index| self.points.get(index))
            .copied()
    }

    /// Every point, by index.
    pub(crate) fn points(&self) -> &[SourcePoint] {
        &self.points
    }

    /// The calls `point` was inlined through, innermost first.
    pub(crate) fn call_chain(&self, point: SourcePoint) -> Vec<SourcePosition> {
        let mut chain = Vec::new();
        let mut call = point.inlined_at;
        while let Some(index) = call {
            let Some(site) = usize::try_from(index)
                .ok()
                .and_then(|index| self.points.get(index))
            else {
                break;
            };
            chain.push(SourcePosition {
                source: site.source,
                offset: site.offset,
            });
            call = site.inlined_at;
        }
        chain
    }
}

/// The location that indexes entry `index` of a table, `None` past the last
/// index a location can hold. Cranelift keeps a location in 31 bits, the
/// largest of them the default location's, which stands for no location at
/// all, so every index below the default's is one.
fn location_for(index: usize) -> Option<SourceLoc> {
    let bits = u32::try_from(index).ok()?;
    (bits < SourceLoc::default().bits()).then(|| SourceLoc::new(bits))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(offset: u32, inlined_at: Option<u32>) -> SourcePoint {
        SourcePoint {
            source: SourceId::PROGRAM,
            offset,
            inlined_at,
        }
    }

    #[test]
    fn a_point_is_listed_once_and_read_back_from_its_location() {
        let mut points = SourcePoints::default();
        let first = points.location_of(point(4, None));
        let second = points.location_of(point(9, None));
        assert_eq!(first, Some(SourceLoc::new(0)));
        assert_eq!(second, Some(SourceLoc::new(1)));
        assert_eq!(points.location_of(point(4, None)), first);
        assert_eq!(points.point_at(SourceLoc::new(1)), Some(point(9, None)));
        assert_eq!(points.point_at(SourceLoc::new(2)), None);
        assert_eq!(points.point_at(SourceLoc::default()), None);
    }

    #[test]
    fn a_call_chain_runs_innermost_first_and_a_forward_call_site_is_refused() {
        let mut points = SourcePoints::default();
        let outer = points.location_of(point(1, None)).map(|l| l.bits());
        let middle = points.location_of(point(2, outer)).map(|l| l.bits());
        let inner = point(3, middle);
        assert!(points.location_of(inner).is_some());
        let offsets: Vec<u32> = points.call_chain(inner).iter().map(|p| p.offset).collect();
        assert_eq!(offsets, vec![2, 1]);
        assert_eq!(points.location_of(point(5, Some(7))), None);
    }

    #[test]
    fn no_location_indexes_past_the_last_one_a_location_can_hold() {
        assert_eq!(location_for(0), Some(SourceLoc::new(0)));
        let past = SourceLoc::default().bits();
        let last = past - 1;
        assert_eq!(location_for(last as usize), Some(SourceLoc::new(last)));
        assert_eq!(location_for(past as usize), None);
        assert_eq!(location_for(u32::MAX as usize), None);
        assert_eq!(location_for(usize::MAX), None);
    }
}
