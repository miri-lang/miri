// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a closure trampoline is identified by: how it hands each argument on
//! and what it stores as the result.
//!
//! A runtime function takes a closure together with the address of a
//! trampoline, and calls the trampoline with the closure, the address of each
//! argument's bytes and the address its result goes to. The trampoline loads
//! what the closure takes out of each address, calls the closure and stores
//! what it answers. Two closures whose parameters are passed alike share one
//! trampoline, so the shape — not the closure's types — is what names it.

use std::fmt::{self, Write};

/// One word a trampoline loads out of an argument's bytes, or stores as the
/// result: a scalar of the width it names, or a pointer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrampolineWord {
    I8,
    I16,
    I32,
    I64,
    I128,
    F32,
    F64,
    /// A pointer-sized word: a reference, a type parameter or a pointer-sized
    /// integer.
    Pointer,
}

impl TrampolineWord {
    /// The number of bytes the word occupies, on a target whose pointer is
    /// `pointer_bytes` wide.
    pub fn bytes(self, pointer_bytes: i64) -> i64 {
        match self {
            TrampolineWord::I8 => 1,
            TrampolineWord::I16 => 2,
            TrampolineWord::I32 | TrampolineWord::F32 => 4,
            TrampolineWord::I64 | TrampolineWord::F64 => 8,
            TrampolineWord::I128 => 16,
            TrampolineWord::Pointer => pointer_bytes,
        }
    }

    fn token(self) -> &'static str {
        match self {
            TrampolineWord::I8 => "i8",
            TrampolineWord::I16 => "i16",
            TrampolineWord::I32 => "i32",
            TrampolineWord::I64 => "i64",
            TrampolineWord::I128 => "i128",
            TrampolineWord::F32 => "f32",
            TrampolineWord::F64 => "f64",
            TrampolineWord::Pointer => "ptr",
        }
    }
}

/// How a trampoline hands one argument on to the closure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum TrampolineArgument {
    /// Loads one word out of the argument's bytes and passes it.
    Word(TrampolineWord),
    /// Passes the address of the argument's bytes itself.
    Address,
}

impl TrampolineArgument {
    fn token(self) -> &'static str {
        match self {
            TrampolineArgument::Word(word) => word.token(),
            TrampolineArgument::Address => "addr",
        }
    }
}

/// How a trampoline hands each argument on, in order, and the word it stores
/// the closure's result as, `None` when the closure answers nothing.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TrampolineShape {
    pub arguments: Vec<TrampolineArgument>,
    pub result: Option<TrampolineWord>,
}

/// The token of a result that is nothing.
const NO_RESULT: &str = "void";

impl TrampolineShape {
    /// The tokens of the arguments, each after `separator`, then the result
    /// token after `result_separator`: `$i64$addr.i64` in a link name.
    pub(super) fn write_tokens(
        &self,
        f: &mut fmt::Formatter<'_>,
        separator: &str,
        result_separator: &str,
    ) -> fmt::Result {
        self.arguments.iter().try_for_each(|argument| {
            f.write_str(separator)?;
            f.write_str(argument.token())
        })?;
        f.write_str(result_separator)?;
        f.write_str(self.result.map_or(NO_RESULT, TrampolineWord::token))
    }

    /// The tokens as a reader is shown them: `(i64, addr) -> i64`.
    pub(super) fn write_written(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_char('(')?;
        for (index, argument) in self.arguments.iter().enumerate() {
            if index > 0 {
                f.write_str(", ")?;
            }
            f.write_str(argument.token())?;
        }
        write!(
            f,
            ") -> {}",
            self.result.map_or(NO_RESULT, TrampolineWord::token)
        )
    }
}
