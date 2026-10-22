// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use std::fmt;

/// The source file a [`crate::error::Span`]'s byte offsets index.
///
/// The program's entry file is [`SourceId::PROGRAM`]; every module it imports
/// is numbered from 1 in the order the type checker finished loading it, which
/// a compilation of the same sources repeats exactly.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SourceId(usize);

impl SourceId {
    /// The program's entry file.
    pub const PROGRAM: SourceId = SourceId(0);

    /// The source numbered `index`.
    pub fn new(index: usize) -> Self {
        Self(index)
    }

    /// The position of this source in a table indexed by [`SourceId`].
    pub fn index(self) -> usize {
        self.0
    }
}

impl fmt::Display for SourceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}
