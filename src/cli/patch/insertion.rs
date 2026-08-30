// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Working out where a new declaration goes, and at what depth.
//!
//! Two source-of-truth strategies live here, and they are not
//! interchangeable. A **function** is located through the token
//! correspondence its canonical rendering has with the file, which is exact.
//! A **container** cannot be: the parser records a plain field `total int` as
//! mutable, so the renderer writes `var total int` and the canonical form
//! parts from the file on the first class that declares a field. A container's
//! body is therefore read off the layout instead, and the offset that comes
//! out is put past the lexer so that a token spanning a line break can never
//! be spliced through.

use crate::ast::formatter;
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::cli::{resolve, token_align};
use crate::error::diagnostic::Diagnostic;
use crate::error::syntax::Span;
use crate::lexer::Lexer;

use super::{
    anchor_in_another_scope, bodyless_container, container_missing, declared_something_else,
    not_anchorable, parse,
};

/// Where a new declaration goes, and at what depth.
pub(super) struct Placement {
    /// The byte offset the new text is spliced in at.
    pub(super) at: usize,
    /// The indentation every line of the new declaration carries.
    pub(super) indent: String,
    /// Whether the new text needs a blank line before it.
    ///
    /// False only for a file with nothing in it, where a leading blank line
    /// would open the result with whitespace nobody wrote.
    pub(super) separated: bool,
}

/// Work out where a new declaration goes.
///
/// A named anchor puts it after that declaration, as its sibling. A method
/// with no anchor goes at the end of its container's body. Anything else is
/// appended to the file.
pub(super) fn place(
    source: &str,
    program: &Program,
    container: Option<&str>,
    after: Option<&str>,
) -> Result<Placement, Box<Diagnostic>> {
    match (after, container) {
        (Some(anchor), _) => beside(source, program, container, anchor),
        (None, Some(name)) => among_members(source, program, name),
        (None, None) => Ok(appended(source)),
    }
}

/// Place a declaration after the one an anchor names, as its sibling.
fn beside(
    source: &str,
    program: &Program,
    container: Option<&str>,
    anchor: &str,
) -> Result<Placement, Box<Diagnostic>> {
    let declaration = resolve::resolve(program, anchor)?;
    // The new declaration becomes the anchor's sibling, so the two have to
    // belong to the same place. Without this the insert still refuses, but it
    // refuses after the splice, describing the wrong problem.
    let holder = holding_container(program, declaration);
    if holder.as_deref() != container {
        return Err(anchor_in_another_scope(
            anchor,
            container,
            holder.as_deref(),
        ));
    }
    let (_, end) = declaration_extent(source, declaration, anchor)?;
    Ok(Placement {
        at: end,
        indent: line_indent(source, name_offset(declaration).unwrap_or(end)),
        separated: true,
    })
}

/// Place a method among the members its container already declares.
fn among_members(
    source: &str,
    program: &Program,
    container: &str,
) -> Result<Placement, Box<Diagnostic>> {
    let declaration =
        container_named(program, container).ok_or_else(|| container_missing(container))?;
    let header = declared_name_span(declaration)
        .map(|span| span.start)
        .ok_or_else(|| not_anchorable(container))?;
    let body = container_body(source, header).ok_or_else(|| bodyless_container(container))?;
    refuse_offset_inside_token(source, body.end, container)?;
    Ok(Placement {
        at: body.end,
        indent: body.indent,
        separated: true,
    })
}

/// Place a declaration at the end of the file.
///
/// It goes after the last byte that carries anything, so the file keeps
/// whatever trailing whitespace its author left on the end.
fn appended(source: &str) -> Placement {
    let at = source.trim_end().len();
    Placement {
        at,
        indent: String::new(),
        separated: at > 0,
    }
}

/// The byte range a declaration occupies in the source it was parsed from.
///
/// The range comes from the token correspondence rather than from the
/// statement's span, because the parser records a span only for a declared
/// name: a class or struct statement carries an empty one. Aligning the
/// canonical rendering against the file also proves the declaration is
/// anchorable at all, which is what makes the end offset trustworthy enough
/// to splice at.
fn declaration_extent(
    source: &str,
    declaration: &Statement,
    name: &str,
) -> Result<(usize, usize), Box<Diagnostic>> {
    let span = declared_name_span(declaration).ok_or_else(|| not_anchorable(name))?;
    // The name comes from the span the parser recorded rather than from the
    // AST, so it is one identifier token even where the declaration renders
    // its name with generic arguments beside it.
    let declared = source
        .get(span.start..span.end)
        .ok_or_else(|| not_anchorable(name))?;
    let rendered = formatter::declaration(declaration);
    let alignment = token_align::build_alignment(source, &rendered.text, declared, span)
        .map_err(|diverged| Box::new(diverged.to_diagnostic()))?;
    alignment.raw_extent().ok_or_else(|| not_anchorable(name))
}

/// The span the parser recorded for a declaration's own name.
fn declared_name_span(declaration: &Statement) -> Option<Span> {
    if let StatementKind::FunctionDeclaration(data) = &declaration.node {
        return Some(data.name_span);
    }
    resolve::container_name_expression(declaration).map(|name| name.span)
}

/// Where a declaration's own name starts in the file.
fn name_offset(declaration: &Statement) -> Option<usize> {
    declared_name_span(declaration).map(|span| span.start)
}

/// The container that holds a declaration, or nothing when it is top level.
fn holding_container(program: &Program, declaration: &Statement) -> Option<String> {
    program.body.iter().find_map(|statement| {
        let name = resolve::container_name(statement)?;
        resolve::children(statement)
            .into_iter()
            .any(|member| std::ptr::eq(member, declaration))
            .then_some(name)
    })
}

/// Check the inserted text declares the one thing it was asked for.
///
/// A method joins a container without changing what the file declares at the
/// top level, so text carrying a second declaration would otherwise arrive
/// unremarked and the caller would be told it inserted one thing.
pub(super) fn confirm_text_declares_only(text: &str, name: &str) -> Result<(), Box<Diagnostic>> {
    // Measured at the caller's own indentation, which is what is spliced in.
    let parsed = parse(&indented(text, "", "\n"))?;
    let bare = name.rsplit_once('.').map_or(name, |(_, method)| method);
    match parsed.body.as_slice() {
        [only] if resolve::declared_function_name(only) == Some(bare) => Ok(()),
        _ => Err(declared_something_else(name)),
    }
}

/// The container of that name, if the file declares one.
fn container_named<'a>(program: &'a Program, name: &str) -> Option<&'a Statement> {
    program
        .body
        .iter()
        .find(|statement| resolve::container_name(statement).is_some_and(|found| found == name))
}

/// Whether the file already declares this name where the new one would go.
///
/// A method is looked for in its own container only, and a bare name at the
/// top level only, so a top-level `total` and a method `Order.total` do not
/// collide: they are different declarations and the file may hold both.
pub(super) fn already_declared(program: &Program, name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((container, method)) => !resolve::methods_of(program, container, method).is_empty(),
        None => program
            .body
            .iter()
            .any(|statement| resolve::declared_function_name(statement) == Some(name)),
    }
}

/// The leading whitespace of the line holding `offset`.
fn line_indent(source: &str, offset: usize) -> String {
    let start = source
        .get(..offset)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |index| index + 1);
    source
        .get(start..offset)
        .unwrap_or_default()
        .chars()
        .take_while(|c| *c == ' ' || *c == '\t')
        .collect()
}

/// A container's body: where it ends, and what depth its members sit at.
struct ContainerBody {
    /// The byte just past the last thing the body declares.
    end: usize,
    /// The indentation the container's own members carry.
    indent: String,
}

/// Read a container's body off the layout the author wrote.
///
/// A container renders its plain fields with the `var` the parser inferred for
/// them, so its canonical form does not agree token-for-token with the file and
/// the alignment that serves a function cannot serve a container. What is
/// reliable is the layout: this language delimits a body by indentation, so the
/// body is the run of lines below the header that are indented past it, and the
/// members are the shallowest of those. Blank lines inside the run belong to it.
fn container_body(source: &str, header_offset: usize) -> Option<ContainerBody> {
    let header_line = source
        .get(..header_offset)?
        .rfind('\n')
        .map_or(0, |at| at + 1);
    let header = indent_width(source.get(header_line..)?);

    let mut offset = header_line;
    let mut end = None;
    let mut indent: Option<String> = None;
    for line in source.get(header_line..)?.split('\n') {
        let is_header = offset == header_line;
        let blank = line.trim().is_empty();
        if !is_header && !blank {
            let width = indent_width(line);
            if width <= header {
                break;
            }
            end = Some(offset + line.trim_end().len());
            if indent.as_ref().is_none_or(|found| width < found.len()) {
                indent = Some(line.chars().take(width).collect());
            }
        }
        // `split` took the newline out, so putting one byte back tracks the
        // file. A `\r` before it is part of `line.len()` already.
        offset += line.len() + 1;
    }

    Some(ContainerBody {
        end: end?,
        indent: indent?,
    })
}

/// How deep a line is indented.
fn indent_width(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Refuse an insertion point that falls inside a token.
///
/// A body read off the layout is only as good as the assumption that a line
/// break is a boundary. A token that spans one — a literal written across
/// several lines — would put the offset inside it, and splicing there would cut
/// the token in half. The lexer is what knows, so it is asked.
fn refuse_offset_inside_token(
    source: &str,
    offset: usize,
    name: &str,
) -> Result<(), Box<Diagnostic>> {
    for token in Lexer::new(source).flatten() {
        let (_, span) = token;
        if span.start < offset && offset < span.end {
            return Err(not_anchorable(name));
        }
    }
    Ok(())
}

/// Re-indent a declaration to sit at `indent`, on `ending` line endings.
///
/// The caller's own indentation is measured and removed first, so text written
/// flush left and text copied out of a class body both land at the same depth.
pub(super) fn indented(text: &str, indent: &str, ending: &str) -> String {
    let trimmed = text.trim_end_matches(['\n', '\r']);
    let common = trimmed
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    trimmed
        .lines()
        .map(|line| {
            let stripped = line.get(common..).unwrap_or("").trim_end_matches('\r');
            if stripped.trim().is_empty() {
                String::new()
            } else {
                format!("{}{}", indent, stripped)
            }
        })
        .collect::<Vec<_>>()
        .join(ending)
}

/// The line ending the file is written with.
///
/// An insert introduces separators of its own, and a file that uses one ending
/// throughout must not come back carrying two.
pub(super) fn dominant_line_ending(source: &str) -> &'static str {
    if source.contains("\r\n") {
        "\r\n"
    } else {
        "\n"
    }
}
