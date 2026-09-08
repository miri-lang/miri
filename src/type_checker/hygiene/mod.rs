// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What an edit leaves behind.
//!
//! These checks look for code that is not wrong: a binding nothing reads, a
//! parameter no body uses, an import nothing needs, a private declaration
//! nothing calls, a statement written after the one that leaves. Each names
//! something the program would mean exactly the same without, which is why
//! every one of them is a warning and none of them stops a build.
//!
//! They run over the file being compiled and nothing else. An imported module
//! is somebody else's file: its private helpers are called from inside it, and
//! reporting on it would report a program the author did not write.
//!
//! Every check errs the same way. Where a name could be read either as a use or
//! as something else, it counts as a use — a warning that is never raised costs
//! a reader nothing, and a warning about a name the program really uses costs
//! them the trust they had in the rest.

mod bindings;
mod declarations;
mod imports;
mod names;
mod reachability;
mod walk;

use crate::ast::Program;
use crate::type_checker::TypeChecker;

pub(crate) use walk::exported_names as exported_names_of;

impl TypeChecker {
    /// Reports the residue in `program`: what it declares and never uses, and
    /// what it writes where nothing runs.
    ///
    /// Runs only when nothing else has been reported. A file that does not
    /// type-check has a tree the checks would read as a program, and the
    /// unused halves of a half-finished edit are not news to someone already
    /// holding an error about it.
    pub(crate) fn check_hygiene(&mut self, program: &Program) {
        if !self.diagnostics.is_empty() {
            return;
        }
        let contents = walk::contents(&program.body);
        // Two of the checks ask the same question of the same statements —
        // which names does this file read anywhere — so they are answered once.
        let file_references = names::References::of_statements(&program.body);
        self.report_unused_locals(&contents);
        self.report_unused_parameters(&contents);
        self.report_unused_imports(program, &file_references);
        self.report_unused_private_declarations(&contents, &file_references);
        self.report_unreachable_statements(&contents);
    }
}

/// True when a name says outright that it is not meant to be read.
///
/// A leading underscore is the spelling that turns off every report about a
/// binding: it is what to write when a signature is fixed from outside and this
/// body has no use for the value.
pub(crate) fn is_deliberately_unread(name: &str) -> bool {
    name.starts_with('_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_leading_underscore_says_the_name_is_not_read() {
        assert!(is_deliberately_unread("_"));
        assert!(is_deliberately_unread("_unused"));
    }

    #[test]
    fn test_an_ordinary_name_says_nothing_of_the_kind() {
        assert!(!is_deliberately_unread("unused"));
        assert!(!is_deliberately_unread("value_"));
    }
}
