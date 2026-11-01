// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a runtime entry point does with each argument it is handed and with
//! the value it hands back.
//!
//! A runtime entry point has no Miri body the compiler could read, so what it
//! does to its arguments is stated where it is declared: as attributes on its
//! `runtime` declaration, or, for an entry point only the compiler calls, in
//! the compiler's own table of them. Either way one [`RuntimeEffect`] per entry
//! point records it, and every stage that reasons about a runtime call reads
//! that record rather than recognising the entry point by its name.
//!
//! A record states every fact. An argument that holds a reference has a stated
//! use; nothing is assumed of an entry point the record does not describe.

use std::collections::HashMap;
use std::rc::Rc;

/// What every `runtime` declaration states, keyed by the entry point it
/// declares.
pub type RuntimeEffects = HashMap<String, Rc<RuntimeEffect>>;

/// What an element argument of a collection entry point is to the
/// collection it is handed to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ElementRole {
    /// An element of a list, an array or a set.
    Element,
    /// A key of a map.
    Key,
    /// A value of a map.
    Value,
}

/// What a runtime entry point that calls a Miri closure promises about the
/// call: how many arguments it passes, and where they come from. The closure
/// always answers in a pointer-sized `int`, which the runtime reads the sign
/// of.
///
/// The runtime calls a closure through a trampoline the compiler generates
/// for the closure's parameter types (see `mir::closure_abi`), handing it the
/// address of each argument's bytes. Those addresses are element slots of a
/// collection argument, so a closure's parameters must be read exactly as
/// that collection holds its elements; a declaration that promises any other
/// shape is refused where it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ClosureContract {
    /// How many arguments the runtime calls the closure with.
    pub arity: usize,
    /// The parameter position of the collection whose elements each argument
    /// is; `None` for a closure the runtime calls with none.
    pub elements_of: Option<usize>,
}

/// What an entry point does with one declared parameter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ParamUse {
    /// A number or a flag: a plain word, with no count to keep.
    Scalar,
    /// Reads what it is handed and keeps no reference to it past the call.
    Reads,
    /// Writes what it is handed in place, keeping no reference to it past
    /// the call: the count the value had going in is the count it leaves with.
    Writes,
    /// An element the collection keeps: the runtime takes the caller's
    /// reference over, and the element travels by address.
    Stores(ElementRole),
    /// An element the collection's contents are matched against: read, never
    /// kept, and travelling by address like a stored one.
    LooksUp(ElementRole),
    /// A closure the runtime calls in the shape the contract gives.
    Closure(ClosureContract),
    /// The receiver of a copy-on-write entry point: taken over and handed
    /// back, itself when nothing else holds it and a copy otherwise.
    CopiesOnWrite,
    /// The receiver of a string append: taken over, written in place, and
    /// handed back as the result.
    Appends,
    /// A value whose count is read and nothing else: whether it is shared.
    CountOf,
}

/// What an entry point hands back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ResultUse {
    /// Nothing, a plain word, or a value its caller owns outright.
    Owned,
    /// A collection it has just allocated, empty, whose count is 1.
    Allocates,
    /// A copy of the collection at the parameter position: allocated, held
    /// by the caller alone, as long as the original, and written without
    /// touching the header of any collection that already exists.
    Copies(usize),
    /// An element, written into storage the caller names by two trailing
    /// arguments, its address and its byte count, rather than returned.
    Element,
    /// An element written the way [`ResultUse::Element`] is, which for a
    /// counted element is a reference the container keeps owning: valid
    /// until the container is next written, and never released by the caller.
    BorrowedElement,
}

/// Everything a runtime entry point does to what it is handed and to what
/// it hands back.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct RuntimeEffect {
    /// One use per declared parameter, in declaration order.
    pub params: Vec<ParamUse>,
    /// What the entry point hands back.
    pub result: ResultUse,
    /// Never returns: reports a failure and ends the process, or unwinds past
    /// the call through the trap the testing harness installs.
    pub diverges: bool,
    /// Orders the elements of the container at this position, reading each
    /// element's bytes as a number.
    pub orders: Option<usize>,
    /// Matches values against the elements, or keys, of the container at
    /// this position, by the rule the compiler registered for their type.
    pub matches: Option<usize>,
    /// Reads a count or bumps a statistic and reaches no code that could
    /// grow, shrink, replace or release any collection.
    pub leaves_headers: bool,
    /// Reads the count of what it is handed: asks whether it is shared, or
    /// stops the program unless the call holds it alone.
    pub reads_count: bool,
}

impl RuntimeEffect {
    /// An entry point that takes the given parameters and hands back a value
    /// its caller owns, with no other effect.
    pub fn new(params: Vec<ParamUse>) -> Self {
        RuntimeEffect {
            params,
            result: ResultUse::Owned,
            diverges: false,
            orders: None,
            matches: None,
            leaves_headers: false,
            reads_count: false,
        }
    }

    /// The use of the declared parameter `position`; a position past the
    /// declared parameters is a plain word.
    pub fn param(&self, position: usize) -> ParamUse {
        self.params
            .get(position)
            .copied()
            .unwrap_or(ParamUse::Scalar)
    }

    /// Positions whose reference the entry point takes over, in call order:
    /// a stored element, a copy-on-write receiver, or a string append's
    /// receiver. The caller stops owning what it passed there.
    pub fn taken_positions(&self) -> Vec<usize> {
        self.positions(|use_| {
            matches!(
                use_,
                ParamUse::Stores(_) | ParamUse::CopiesOnWrite | ParamUse::Appends
            )
        })
    }

    /// Whether the entry point keeps no reference to what it is handed at
    /// `position` and writes nothing reached through it.
    pub fn only_reads(&self, position: usize) -> bool {
        matches!(
            self.param(position),
            ParamUse::Scalar | ParamUse::Reads | ParamUse::LooksUp(_)
        )
    }

    /// Positions carrying an element the container keeps, in ascending
    /// order: the only arguments a call to the entry point lets escape.
    pub fn stored_positions(&self) -> Vec<usize> {
        self.positions(|use_| matches!(use_, ParamUse::Stores(_)))
    }

    /// Whether the entry point writes, in place, what it is handed at
    /// `position`.
    pub fn writes_in_place(&self, position: usize) -> bool {
        matches!(self.param(position), ParamUse::Writes | ParamUse::Appends)
    }

    /// Whether this is the copy-on-write entry point of a collection.
    pub fn is_copy_on_write(&self) -> bool {
        self.params.contains(&ParamUse::CopiesOnWrite)
    }

    /// Whether the entry point asks whether the value it is handed is shared,
    /// reading its count and nothing else.
    pub fn asks_whether_shared(&self) -> bool {
        self.params.first() == Some(&ParamUse::CountOf)
    }

    /// Whether the entry point stops the program unless the call holds what
    /// it is handed alone, reading its count and nothing else.
    pub fn insists_on_unique(&self) -> bool {
        self.reads_count && !self.asks_whether_shared()
    }

    /// The positions of the key and the value a map entry point stores as
    /// one entry; `None` when it stores no entry.
    pub fn stored_entry(&self) -> Option<(usize, usize)> {
        let at = |role| (self.params.iter()).position(|use_| *use_ == ParamUse::Stores(role));
        at(ElementRole::Key).zip(at(ElementRole::Value))
    }

    /// Whether the entry point hands back a value nothing else holds: a
    /// block it has just allocated or copied, or what a copy-on-write hands
    /// back — its argument when its count was 1, a copy otherwise.
    pub fn hands_back_unshared(&self) -> bool {
        self.returns_copy() || self.allocates_collection() || self.is_copy_on_write()
    }

    /// Whether the entry point leaves the count of what it is handed at
    /// `position` as it found it: it writes the argument in place and does
    /// not take it over.
    pub fn keeps_count_of(&self, position: usize) -> bool {
        self.param(position) == ParamUse::Writes
    }

    /// Whether what the entry point hands back is a block nothing else holds,
    /// copied from a collection it is handed.
    pub fn returns_copy(&self) -> bool {
        matches!(self.result, ResultUse::Copies(_))
    }

    /// Whether the entry point builds an empty collection.
    pub fn allocates_collection(&self) -> bool {
        self.result == ResultUse::Allocates
    }

    /// Whether what the entry point hands back has exactly as many elements
    /// as the collection it is handed: a copy, or a copy-on-write.
    pub fn preserves_length(&self) -> bool {
        self.returns_copy() || self.is_copy_on_write()
    }

    /// Whether the entry point hands back the collection it is handed, or a
    /// fresh copy of it, writing the header of no collection that already
    /// exists. The copy may run an element's own `clone` method, which the
    /// caller must rule out from the element type.
    pub fn copies_collection(&self) -> bool {
        self.preserves_length()
    }

    /// Positions carrying an element the container stores or looks up, each
    /// with what it is to the container.
    pub fn element_roles(&self) -> Vec<(usize, ElementRole)> {
        (self.params.iter().enumerate())
            .filter_map(|(position, use_)| match use_ {
                ParamUse::Stores(role) | ParamUse::LooksUp(role) => Some((position, *role)),
                ParamUse::Scalar
                | ParamUse::Reads
                | ParamUse::Writes
                | ParamUse::Closure(_)
                | ParamUse::CopiesOnWrite
                | ParamUse::Appends
                | ParamUse::CountOf => None,
            })
            .collect()
    }

    /// Positions carrying an element, in ascending order.
    pub fn element_positions(&self) -> Vec<usize> {
        (self.element_roles().into_iter())
            .map(|(position, _)| position)
            .collect()
    }

    /// The contract of the closure the runtime calls through `position`;
    /// `None` when it calls none there.
    pub fn closure_contract(&self, position: usize) -> Option<ClosureContract> {
        match self.param(position) {
            ParamUse::Closure(contract) => Some(contract),
            ParamUse::Scalar
            | ParamUse::Reads
            | ParamUse::Writes
            | ParamUse::Stores(_)
            | ParamUse::LooksUp(_)
            | ParamUse::CopiesOnWrite
            | ParamUse::Appends
            | ParamUse::CountOf => None,
        }
    }

    /// Positions carrying a closure, in ascending order.
    pub fn closure_positions(&self) -> Vec<usize> {
        self.positions(|use_| matches!(use_, ParamUse::Closure(_)))
    }

    /// Whether the entry point hands its caller an element through storage
    /// the caller names.
    pub fn returns_element_value(&self) -> bool {
        matches!(self.result, ResultUse::Element | ResultUse::BorrowedElement)
    }

    /// Whether the entry point hands back a reference its container keeps
    /// owning.
    pub fn hands_back_a_borrow(&self) -> bool {
        self.result == ResultUse::BorrowedElement
    }

    fn positions(&self, keep: impl Fn(ParamUse) -> bool) -> Vec<usize> {
        (self.params.iter().enumerate())
            .filter(|(_, use_)| keep(**use_))
            .map(|(position, _)| position)
            .collect()
    }
}
