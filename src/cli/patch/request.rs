// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading the edit flags of one `miri patch` call as a sequence of operations.
//!
//! The flags arrive as parallel lists: a call names its edits by repeating a
//! flag, and the lists pair up by position. Everything here is about turning
//! that shape into operations, or refusing a call whose lists do not describe
//! a coherent edit — before any file is read and before anything is applied.

use super::{coded, text_from_file, Edit, Operation};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::Diagnostic;

/// The edit flags as they arrived, before they are read as operations.
#[derive(Debug, Default, Clone)]
pub struct Request {
    /// Functions named by `--replace-in-fn`.
    pub functions: Vec<String>,
    /// Anchors given inline.
    pub old: Vec<String>,
    /// Replacements given inline.
    pub new: Vec<String>,
    /// Files, or `-`, carrying the anchors.
    pub old_file: Vec<String>,
    /// Files, or `-`, carrying the replacements.
    pub new_file: Vec<String>,
    /// Functions named by `--replace-fn`.
    pub replace_functions: Vec<String>,
    /// Files, or `-`, carrying the replacement bodies.
    pub body_file: Vec<String>,
    /// Declarations named by `--insert-fn`.
    pub insert_functions: Vec<String>,
    /// Declarations the new ones follow; empty, or one per `--insert-fn`.
    pub after: Vec<String>,
}

/// Read the edit flags as a sequence of operations.
///
/// Anchored edits are applied in the order they were written, and the edits
/// that take a body file after them. A batch is applied to one text and checked
/// once, so a later edit sees what an earlier one did.
pub fn operations(request: &Request) -> Result<Vec<Operation>, Box<Diagnostic>> {
    reject_multiple_standard_inputs(request)?;
    let old = one_source_of(&request.old, &request.old_file, "--old", "--old-file")?;
    let new = one_source_of(&request.new, &request.new_file, "--new", "--new-file")?;
    check_pairings(request, old.len(), new.len())?;

    let mut built = Vec::new();
    for ((function, old), new) in request.functions.iter().zip(old).zip(new) {
        built.push(Operation {
            function: function.clone(),
            edit: Edit::Anchored { old, new },
        });
    }

    // One call carries replacements or inserts, never both, so whichever list
    // is populated is the one the body files belong to.
    let mut bodies = request.body_file.iter();
    for function in &request.replace_functions {
        built.push(Operation {
            function: function.clone(),
            edit: Edit::Body {
                text: read_next_body(&mut bodies)?,
            },
        });
    }
    for (index, function) in request.insert_functions.iter().enumerate() {
        built.push(Operation {
            function: function.clone(),
            edit: Edit::Insert {
                text: read_next_body(&mut bodies)?,
                after: request.after.get(index).cloned(),
            },
        });
    }
    Ok(built)
}

/// Check that every list a call pairs up arrived at the same length.
fn check_pairings(request: &Request, old: usize, new: usize) -> Result<(), Box<Diagnostic>> {
    let anchored = request.functions.len();
    paired(anchored, "--replace-in-fn", old, "--old")?;
    paired(anchored, "--replace-in-fn", new, "--new")?;

    if !request.replace_functions.is_empty() && !request.insert_functions.is_empty() {
        return Err(malformed(
            "--replace-fn and --insert-fn were both given; each pairs with --body-file, so one call takes one of them".to_string(),
        ));
    }
    paired(
        request.replace_functions.len() + request.insert_functions.len(),
        "--replace-fn or --insert-fn",
        request.body_file.len(),
        "--body-file",
    )?;

    // An anchor for some inserts and not others names no order for the rest,
    // so a call gives one for every insert or for none.
    if !request.after.is_empty() {
        paired(
            request.insert_functions.len(),
            "--insert-fn",
            request.after.len(),
            "--after",
        )?;
    }
    Ok(())
}

/// Refuse two lists that pair up by position and did not arrive equal.
fn paired(
    left: usize,
    left_flag: &str,
    right: usize,
    right_flag: &str,
) -> Result<(), Box<Diagnostic>> {
    if left == right {
        return Ok(());
    }
    Err(malformed(format!(
        "{} {} against {} {}; they pair up in the order they are written, so a call gives the same number of each",
        left, left_flag, right, right_flag
    )))
}

/// Take the next body file's text.
///
/// The pairing check has already established there is one per edit, so an
/// exhausted list here would mean that check and this loop disagree.
fn read_next_body<'a>(
    bodies: &mut impl Iterator<Item = &'a String>,
) -> Result<String, Box<Diagnostic>> {
    let path = bodies.next().ok_or_else(|| {
        malformed("fewer --body-file arguments than the edits naming them".to_string())
    })?;
    text_from_file(path)
}

/// Take the texts from whichever of the two flags carried them.
///
/// One flag or the other answers for a whole call. Accepting both would leave
/// the order of a batch resting on which flag an edit happened to use.
fn one_source_of(
    inline: &[String],
    files: &[String],
    inline_flag: &str,
    file_flag: &str,
) -> Result<Vec<String>, Box<Diagnostic>> {
    if !inline.is_empty() && !files.is_empty() {
        return Err(malformed(format!(
            "{} and {} were both given; one call takes its text from one of them",
            inline_flag, file_flag
        )));
    }
    if inline.is_empty() {
        return files.iter().map(|path| text_from_file(path)).collect();
    }
    Ok(inline.to_vec())
}

/// Refuse a call that would read standard input more than once.
fn reject_multiple_standard_inputs(request: &Request) -> Result<(), Box<Diagnostic>> {
    let from_input = request
        .old_file
        .iter()
        .chain(&request.new_file)
        .chain(&request.body_file)
        .filter(|source| source.as_str() == "-")
        .count();
    if from_input > 1 {
        return Err(malformed(format!(
            "{} arguments read standard input, which can be read once",
            from_input
        )));
    }
    Ok(())
}

/// Report edit flags that do not describe a coherent edit.
pub(super) fn malformed(detail: String) -> Box<Diagnostic> {
    Box::new(coded(
        DiagnosticCode::BldMalformedEditRequest,
        detail,
        "name one function, one anchor and one replacement per edit; repeat the three together to batch edits",
    ))
}
