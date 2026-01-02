// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::error::syntax::Span;
use std::hash::{Hash, Hasher};

#[derive(Debug, Clone, Eq)]
pub struct IdNode<T> {
    pub id: usize,
    pub node: T,
    pub span: Span,
}

impl<T: PartialEq> PartialEq for IdNode<T> {
    fn eq(&self, other: &Self) -> bool {
        self.node == other.node
    }
}

impl<T: Hash> Hash for IdNode<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.node.hash(state);
    }
}

impl<T> IdNode<T> {
    pub fn new(id: usize, node: T, span: Span) -> Self {
        Self { id, node, span }
    }
}
