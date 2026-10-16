// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Sets of places known unshared, one bit per place the analysis tracks.

/// The dense number [`super::places::PlaceTable`] gives a place it tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct FactId(pub(super) usize);

impl FactId {
    fn word(self) -> usize {
        self.0 / u64::BITS as usize
    }

    fn bit(self) -> u64 {
        1 << (self.0 % u64::BITS as usize)
    }
}

/// The places known to hold a value nothing else holds.
#[derive(Debug, Clone)]
pub(super) struct Facts(Vec<u64>);

impl Facts {
    /// The empty set over `ids` tracked places.
    pub(super) fn empty(ids: usize) -> Self {
        Self(vec![0; ids.div_ceil(u64::BITS as usize)])
    }

    pub(super) fn contains(&self, id: FactId) -> bool {
        self.0
            .get(id.word())
            .is_some_and(|word| word & id.bit() != 0)
    }

    pub(super) fn insert(&mut self, id: FactId) {
        if let Some(word) = self.0.get_mut(id.word()) {
            *word |= id.bit();
        }
    }

    /// Drops every fact `mask` names.
    pub(super) fn forget(&mut self, mask: &Mask) {
        let words = self.0.iter_mut().skip(mask.first_word);
        for (word, forgotten) in words.zip(&mask.words) {
            *word &= !forgotten;
        }
    }

    /// Keeps only the facts `other` holds too. Returns true if any was dropped.
    pub(super) fn meet(&mut self, other: &Facts) -> bool {
        let mut changed = false;
        for (word, kept) in self.0.iter_mut().zip(&other.0) {
            let met = *word & kept;
            changed |= met != *word;
            *word = met;
        }
        changed
    }
}

/// A set of facts to forget, stored as the run of words its lowest and
/// highest ids span: the places one read or write reaches are rooted at one
/// local, and a local's places are numbered consecutively, so the run is short.
#[derive(Debug)]
pub(super) struct Mask {
    first_word: usize,
    words: Vec<u64>,
}

impl Mask {
    /// The mask naming `ids`; `None` when there are none.
    pub(super) fn of(ids: impl IntoIterator<Item = FactId>) -> Option<Self> {
        let ids: Vec<FactId> = ids.into_iter().collect();
        let first_word = ids.iter().map(|id| id.word()).min()?;
        let last_word = ids.iter().map(|id| id.word()).max()?;
        let mut words = vec![0; last_word - first_word + 1];
        for id in ids {
            if let Some(word) = words.get_mut(id.word() - first_word) {
                *word |= id.bit();
            }
        }
        Some(Self { first_word, words })
    }
}
