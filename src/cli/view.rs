// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri view` command: read part of a program instead of all of it.
//!
//! The command is split the way [`crate::cli::check`] is: [`view`] does the work
//! and returns what it found without printing, so a long-lived server can call
//! it, and [`run`] adds the writing for the command line.
//!
//! Most shapes render from the parsed AST rather than slicing the file, so the
//! text a tool reads here is the same text it reads next time and the spans
//! that come with it index that text. The source is parsed exactly as written —
//! no script-mode wrapping and no type normalization — so an outline never
//! lists a `main` the author did not write, and a type written `[int]` reads
//! back as `[int]`.
//!
//! That rendering is canonical, which is not the same as literal: a type the
//! file spells `List<String>` reads back `[String]`, and comments are gone. A
//! literal read answers with the file's own bytes instead, each line behind its
//! source line number, for a caller that has to cite or anchor against what the
//! file actually says. It is the one shape that runs before the parse, because
//! a file that will not parse is when a reader most needs to see what it holds.

use std::path::Path;

use crate::ast::common::MemberVisibility;
use crate::ast::doc_comments::DocComments;
use crate::ast::extent;
use crate::ast::formatter::{self, Rendered};
use crate::ast::statement::StatementKind;
use crate::ast::{Program, Statement};
use crate::cli::{coded, resolve, sanitize_for_terminal, serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{
    DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonView, JsonViewSpan,
};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{to_json, Diagnostic, Reportable};
use crate::error::format::format_diagnostic_with_color;
use crate::error::syntax::find_line_info;
use crate::error::type_error::TypeError;
use crate::lexer::Lexer;
use crate::parser::Parser;

/// Which part of a program to read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Shape {
    /// One function, whole or narrowed to the block holding some text.
    Function {
        /// The function's name, or `Class.method` for a method.
        name: String,
        /// Text that narrows the read to the innermost block containing it.
        around: Option<String>,
        /// Return the file's own bytes rather than the canonical rendering.
        literal: bool,
    },
    /// Every declaration's signature, with no bodies.
    Outline { public_only: bool },
    /// Every member callable on one type, its own and those it inherits.
    Members {
        type_name: String,
        public_only: bool,
    },
    /// The whole file, exactly as it is written.
    File,
}

impl Shape {
    /// The name this shape carries in the JSON envelope.
    ///
    /// A literal read carries a different name from the canonical read of the
    /// same region: the two return different text for the same request, and a
    /// consumer holding only the envelope has nothing else to tell them apart.
    fn label(&self) -> &'static str {
        match self {
            Shape::Function {
                around: Some(_),
                literal: false,
                ..
            } => "around",
            Shape::Function {
                around: Some(_),
                literal: true,
                ..
            } => "around-raw",
            Shape::Function {
                around: None,
                literal: false,
                ..
            } => "fn",
            Shape::Function {
                around: None,
                literal: true,
                ..
            } => "fn-raw",
            Shape::Outline { .. } => "outline",
            Shape::Members { .. } => "type",
            Shape::File => "raw",
        }
    }
}

/// The shape a set of `miri view` flags asks for.
///
/// Clap admits at most one mode flag, so what arrived decides the shape. The
/// choice lives here rather than at each call site so that a shape added later
/// is decided once, in the module that owns the shapes.
pub fn shape_for(
    fn_name: Option<String>,
    around: Option<String>,
    public: bool,
    literal: bool,
) -> Shape {
    match fn_name {
        Some(name) => Shape::Function {
            name,
            around,
            literal,
        },
        // A literal read with nothing to narrow it is the whole file, which is
        // the one shape that needs no parse.
        None if literal => Shape::File,
        None => Shape::Outline {
            public_only: public,
        },
    }
}

/// How the command finished, mapped onto a process exit code by the caller.
pub enum Outcome {
    /// The requested source was read back.
    Read,
    /// The request could not be answered.
    Failed,
}

/// What a view read.
pub struct ViewReport {
    /// The envelope, ready to serialize for a machine consumer.
    pub envelope: DiagnosticsEnvelope,
    /// Whether the request was answered.
    pub ok: bool,
    /// The canonical source that was read, empty when the request failed.
    pub text: String,
    /// The diagnostics as the compiler reported them.
    diagnostics: Vec<Diagnostic>,
    /// The source the diagnostics were reported against.
    source: String,
    /// The path the diagnostics were reported against.
    source_path: Option<String>,
}

impl ViewReport {
    /// Render the diagnostics for a person to read.
    pub fn to_pretty(&self, color_mode: ColorMode) -> String {
        self.diagnostics
            .iter()
            .map(|diagnostic| {
                format_diagnostic_with_color(
                    &self.source,
                    diagnostic,
                    self.source_path.as_deref(),
                    color_mode.into(),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Read part of `source` and report what was found.
///
/// Nothing here writes to a stream or ends the process, so the same call serves
/// the command line and a request over a long-lived connection.
pub fn view(path: &Path, source: &str, shape: &Shape) -> ViewReport {
    let source_path = Some(path.display().to_string());
    // A whole-file read needs no parse, and must not want one: a file that
    // will not parse is exactly when a reader needs to see what it says.
    if matches!(shape, Shape::File) {
        return success(shape, whole_file(source), vec![], source, source_path);
    }

    let program = match parse(source) {
        Ok(program) => program,
        Err(diagnostic) => return failure(shape, vec![*diagnostic], source, source_path),
    };

    let read = match shape {
        Shape::Outline { public_only } => Ok(Read::plain(outline(&program, source, *public_only))),
        Shape::Function {
            name,
            around,
            literal,
        } => function_view(&program, source, name, around.as_deref(), *literal),
        // Members are read from the type table rather than from this parse,
        // because inheritance crosses declarations and modules.
        Shape::Members {
            type_name,
            public_only,
        } => return members(Some(path), source, type_name, *public_only),
        Shape::File => unreachable_file_shape(),
    };

    match read {
        Ok(read) => success(shape, read.render, read.notes, source, source_path),
        Err(diagnostic) => failure(shape, vec![*diagnostic], source, source_path),
    }
}

/// A whole-file read is answered before the parse, so this arm cannot run.
///
/// The match stays exhaustive rather than closing over the remaining shapes
/// with a wildcard, so that a shape added later is a compile error here.
fn unreachable_file_shape() -> Result<Read, Box<Diagnostic>> {
    Err(coded(
        DiagnosticCode::BldInputNotReadable,
        "a whole-file read reached the parsing path".to_string(),
        "this is a defect in `miri view`; please report it",
    ))
}

/// What a read produced, and anything the reader should know about it.
struct Read {
    /// The text, and where it came from.
    render: LocatedRender,
    /// Warnings that do not stop the read from answering.
    notes: Vec<Diagnostic>,
}

impl Read {
    /// A read that answered exactly what was asked for.
    fn plain(render: LocatedRender) -> Self {
        Read {
            render,
            notes: Vec::new(),
        }
    }
}

/// Parse exactly what was written: no normalization, no script-mode wrapping.
fn parse(source: &str) -> Result<Program, Box<Diagnostic>> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    parser
        .parse()
        .map_err(|error| Box::new(TypeError::from_syntax_error(&error).to_diagnostic()))
}

/// Render one function, narrowed to a block when an anchor is given.
fn function_view(
    program: &Program,
    source: &str,
    name: &str,
    anchor: Option<&str>,
    literal: bool,
) -> Result<Read, Box<Diagnostic>> {
    let declaration = resolve::resolve(program, name)?;
    // The same rendering `miri patch` aligns against, so an anchor copied
    // out of a read is the text a patch will look for.
    let rendered = formatter::declaration_from_source(declaration, source);
    let Some(anchor) = anchor else {
        let render = if literal {
            literal_region(source, declaration, "function")?
        } else {
            located_declaration(source, declaration, rendered)
        };
        return Ok(Read::plain(render));
    };

    let narrowed = narrow(declaration, source, &rendered, anchor)?;
    // A read that did not narrow returned the declaration, so the span says so
    // rather than calling the whole function a block.
    let kind = if narrowed.reduced {
        "block"
    } else {
        "function"
    };
    let render = if literal {
        literal_region(source, narrowed.node, kind)?
    } else {
        located_block(source, narrowed.node, narrowed.rendered)
    };
    let notes = if narrowed.reduced {
        Vec::new()
    } else {
        vec![*could_not_narrow(anchor, narrowed.reason)]
    };
    Ok(Read { render, notes })
}

/// The file's own bytes for the whole file, numbered from its first line.
fn whole_file(source: &str) -> LocatedRender {
    let text = numbered(source, 0, source.len());
    let (_, end_line, _) = find_line_info(source, source.len());
    LocatedRender {
        spans: vec![LocatedSpan {
            span: formatter::RecordedSpan {
                start: 0,
                end: text.len(),
                kind: "file".to_string(),
                name: None,
            },
            line: Some(1),
            end_line: Some(end_line),
        }],
        text,
    }
}

/// The file's own bytes over the region `node` occupies, numbered.
///
/// The region is widened to whole lines, so the indentation the node sits at
/// in the file comes back with it and the text can be put back where it was.
fn literal_region(
    source: &str,
    node: &Statement,
    kind: &str,
) -> Result<LocatedRender, Box<Diagnostic>> {
    let Some(extent) = extent::source_extent(node) else {
        return Err(no_recorded_extent(kind));
    };
    let text = numbered(source, extent.start, extent.end);
    let (line, _, _) = find_line_info(source, extent.start);
    let (end_line, _, _) = find_line_info(source, extent.end);
    Ok(LocatedRender {
        spans: vec![LocatedSpan {
            span: formatter::RecordedSpan {
                start: 0,
                end: text.len(),
                kind: kind.to_string(),
                name: None,
            },
            line: Some(line),
            end_line: Some(end_line),
        }],
        text,
    })
}

/// The bytes of `source` from `start` to `end`, each line behind its number.
///
/// The region is widened to whole lines first, so what comes back is what the
/// file holds and not a fragment starting mid-line. Each line is prefixed with
/// its 1-based number in the file and a single tab, and nothing else changes:
/// dropping everything up to and including the first tab of each line gives
/// the file's bytes back exactly. A file whose last line has no line ending
/// gains one, because a numbered line has to end somewhere.
fn numbered(source: &str, start: usize, end: usize) -> String {
    let start = source
        .get(..start)
        .and_then(|before| before.rfind('\n'))
        .map_or(0, |at| at + 1);
    let end = match source.get(end..).and_then(|after| after.find('\n')) {
        Some(at) => end + at + 1,
        None => source.len(),
    };
    let (first, _, _) = find_line_info(source, start);
    source
        .get(start..end)
        .unwrap_or_default()
        .split_inclusive('\n')
        .enumerate()
        .map(|(offset, line)| {
            let body = line.strip_suffix('\n').unwrap_or(line);
            format!("{}\t{}\n", first + offset, body)
        })
        .collect()
}

/// Report a node the parser recorded no source extent for.
fn no_recorded_extent(kind: &str) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldSourceNotAnchorable,
        format!("this {} carries no recorded source extent", kind),
        "read the whole file with `miri view <PATH> --raw`, or the canonical rendering by dropping `--raw`",
    )
}

/// Report a read that returned more than the anchor asked to narrow to.
fn could_not_narrow(anchor: &str, reason: &'static str) -> Box<Diagnostic> {
    Box::new(
        crate::error::diagnostic::DiagnosticBuilder::warning(
            DiagnosticCode::BldViewCouldNotNarrow.title().to_string(),
        )
        .code(DiagnosticCode::BldViewCouldNotNarrow.as_str())
        .message(format!(
            "`{}` did not narrow the read: {}",
            sanitize_for_terminal(anchor),
            reason
        ))
        .help(
            "anchor on text inside the block you want, such as a statement within a loop or a branch"
                .to_string(),
        )
        .build(),
    )
}

/// Locate a whole rendered declaration in the file it was read from.
///
/// The declaration occupies the rendering from its first byte, so the span
/// starting there is the declaration itself; a span recorded further in belongs
/// to something nested, which this call has no source statement for.
fn located_declaration(source: &str, declaration: &Statement, rendered: Rendered) -> LocatedRender {
    let (line, end_line) = source_lines(source, declaration);
    LocatedRender {
        text: rendered.text,
        spans: rendered
            .spans
            .into_iter()
            .map(|span| {
                let whole = span.start == 0;
                LocatedSpan {
                    span,
                    line: whole.then_some(line).flatten(),
                    end_line: whole.then_some(end_line).flatten(),
                }
            })
            .collect(),
    }
}

/// Locate a narrowed block in the file it was read from.
///
/// A block is not a declaration, so the formatter records no span for it. One
/// is synthesized over the whole rendering, because the lines a reader needs in
/// order to edit what it just read are exactly the block's own.
fn located_block(source: &str, block: &Statement, rendered: Rendered) -> LocatedRender {
    let (line, end_line) = source_lines(source, block);
    LocatedRender {
        spans: vec![LocatedSpan {
            span: formatter::RecordedSpan {
                start: 0,
                end: rendered.text.len(),
                kind: "block".to_string(),
                name: None,
            },
            line,
            end_line,
        }],
        text: rendered.text,
    }
}

/// The source lines `node` was read from, 1-based and inclusive.
///
/// A declaration is anchored at its name, which is where a reader looking for
/// it starts; anything else is anchored at the first byte of source it covers.
fn source_lines(source: &str, node: &Statement) -> (Option<usize>, Option<usize>) {
    let Some(extent) = extent::source_extent(node) else {
        return (None, None);
    };
    let start = declaration_offset(node).unwrap_or(extent.start);
    let (line, _, _) = find_line_info(source, start);
    let (end_line, _, _) = find_line_info(source, extent.end);
    (Some(line), Some(end_line))
}

/// What narrowing a read to an anchor produced.
struct Narrowed<'a> {
    /// The rendering of the region the read returns.
    rendered: Rendered,
    /// The statement that region came from, so a caller can locate it.
    node: &'a Statement,
    /// Whether the region is smaller than the whole declaration.
    reduced: bool,
    /// Why it is not, when it is not.
    reason: &'static str,
}

/// Narrow a rendered function to the innermost block holding `anchor`.
///
/// Returns the rendering together with the statement it came from, so the
/// caller can report where in the file that block lives, and with whether the
/// read was actually reduced. Handing back the whole function under the name
/// of a narrowed read is the failure worth reporting: a caller that asked to
/// see one branch and silently received all of `main` has no way to tell.
fn narrow<'a>(
    declaration: &'a Statement,
    source: &str,
    rendered: &Rendered,
    anchor: &str,
) -> Result<Narrowed<'a>, Box<Diagnostic>> {
    let occurrences = rendered.text.matches(anchor).count();
    if occurrences == 0 {
        return Err(anchor_not_found(anchor));
    }
    if occurrences > 1 {
        return Err(anchor_not_unique(anchor, occurrences));
    }

    // Exactly one occurrence means every block containing it lies on a single
    // path from the function inwards, so the deepest one that contains it is
    // the innermost. Searching deepest-first stops at that block instead of
    // rendering every block in the function to compare their lengths.
    let innermost = blocks(declaration)
        .into_iter()
        .map(|block| (formatter::declaration_from_source(block, source), block))
        .find(|(text, _)| text.text.contains(anchor));

    Ok(classify(declaration, rendered, innermost))
}

/// Say what a search for the innermost block actually found.
fn classify<'a>(
    declaration: &'a Statement,
    rendered: &Rendered,
    innermost: Option<(Rendered, &'a Statement)>,
) -> Narrowed<'a> {
    // An anchor that matches only the signature belongs to no block; the
    // function itself is then the narrowest honest answer, and saying so is
    // what keeps it from reading as a narrowing.
    let Some((narrowed, node)) = innermost else {
        return Narrowed {
            rendered: rendered.clone(),
            node: declaration,
            reduced: false,
            reason: "no block holds it, so the whole declaration came back",
        };
    };

    // The declaration's own body is every statement in it. Returning it is
    // returning the function, whatever the anchor asked for.
    if is_body_of(declaration, node) {
        return Narrowed {
            rendered: narrowed,
            node,
            reduced: false,
            reason: "it sits at the top level of the body, so the whole body came back",
        };
    }

    Narrowed {
        rendered: narrowed,
        node,
        reduced: true,
        reason: "",
    }
}

/// Whether `block` is the declaration's own body rather than something in it.
fn is_body_of(declaration: &Statement, block: &Statement) -> bool {
    resolve::children(declaration)
        .into_iter()
        .any(|child| std::ptr::eq(child, block))
}

/// Every block statement inside a declaration, deepest first.
///
/// Deepest first is what lets a search stop at its first hit: among blocks that
/// all contain the same text, the deepest is the innermost.
fn blocks(node: &Statement) -> Vec<&Statement> {
    let mut found = Vec::new();
    collect_blocks(node, 0, &mut found);
    found.sort_by_key(|(_, depth)| std::cmp::Reverse(*depth));
    found.into_iter().map(|(block, _)| block).collect()
}

/// Collect block statements with the depth each sits at.
fn collect_blocks<'a>(node: &'a Statement, depth: usize, found: &mut Vec<(&'a Statement, usize)>) {
    if matches!(node.node, StatementKind::Block(_)) {
        found.push((node, depth));
    }
    for child in resolve::children(node) {
        collect_blocks(child, depth + 1, found);
    }
}

/// Render every declaration's signature, each with its first comment line.
///
/// Members are listed under the declaration that holds them, indented, so a
/// reader can find a method without opening the class it belongs to.
fn outline(program: &Program, source: &str, public_only: bool) -> LocatedRender {
    let comments = DocComments::harvest(source);
    let mut outline = Outline {
        comments,
        source,
        text: String::new(),
        spans: Vec::new(),
        public_only,
    };
    outline.write_all(&program.body.iter().collect::<Vec<_>>(), 0);
    LocatedRender {
        text: outline.text,
        spans: outline.spans,
    }
}

/// Canonical text together with each declaration's place in it and in the file.
struct LocatedRender {
    /// The rendered source.
    text: String,
    /// Where each declaration landed, in the rendering and in the file.
    spans: Vec<LocatedSpan>,
}

/// One declaration's span in the rendered text, carrying the source lines it
/// was read from.
///
/// The rendered span lets a reader cite what it just read; the source lines let
/// it go back to the file and edit there. Both are needed, and neither can be
/// derived from the other.
struct LocatedSpan {
    /// Where the declaration sits in the rendered text.
    span: formatter::RecordedSpan,
    /// First line of the declaration in the source file, 1-based.
    line: Option<usize>,
    /// Last line of the declaration in the source file, 1-based and inclusive.
    end_line: Option<usize>,
}

/// Accumulates an outline as it walks a program's declarations.
struct Outline<'a> {
    comments: DocComments,
    source: &'a str,
    text: String,
    spans: Vec<LocatedSpan>,
    public_only: bool,
}

impl Outline<'_> {
    /// Write every declaration among `statements`, and their members.
    fn write_all(&mut self, statements: &[&Statement], depth: usize) {
        for statement in statements {
            if self.should_include(statement) {
                self.write_one(statement, depth);
            }
        }
    }

    /// Check if a statement should be included in the outline.
    /// Whether a declaration belongs in the outline being written.
    ///
    /// A `runtime` declaration binds a symbol in another library and a
    /// non-public member is not callable from outside, so neither is part of
    /// the surface a caller reads. Everything else is either public or not a
    /// declaration at all, and a statement that declares nothing is dropped
    /// later for want of a signature.
    fn should_include(&self, statement: &Statement) -> bool {
        if !self.public_only {
            return true;
        }

        match &statement.node {
            StatementKind::RuntimeFunctionDeclaration(..) => false,
            StatementKind::IntrinsicFunctionDeclaration(_, _, _, _, visibility)
            | StatementKind::Variable(_, visibility)
            | StatementKind::Type(_, visibility)
            | StatementKind::Enum(_, _, _, _, visibility, _)
            | StatementKind::Struct(_, _, _, _, visibility, _)
            | StatementKind::Trait(_, _, _, _, visibility) => is_public(visibility),
            StatementKind::Class(data) => is_public(&data.visibility),
            StatementKind::FunctionDeclaration(declaration) => {
                is_public(&declaration.properties.visibility)
            }
            // Not declarations: the outline drops these for want of a
            // signature, whichever surface was asked for.
            StatementKind::Empty
            | StatementKind::Break
            | StatementKind::Continue
            | StatementKind::Expression(_)
            | StatementKind::Block(_)
            | StatementKind::If(..)
            | StatementKind::While(..)
            | StatementKind::For(..)
            | StatementKind::Forall { .. }
            | StatementKind::GpuFrame(..)
            | StatementKind::GpuFrameBlock(_)
            | StatementKind::Return(_)
            | StatementKind::Use(..) => true,
        }
    }

    /// Write one declaration's signature, then whatever it declares inside.
    fn write_one(&mut self, statement: &Statement, depth: usize) {
        let Some(signature) = formatter::signature(statement) else {
            return;
        };
        let summary = declaration_offset(statement)
            .and_then(|offset| self.comments.summary_before(self.source, offset))
            .map(str::to_string);
        if let Some(summary) = summary {
            self.indent(depth);
            self.text.push_str("// ");
            self.text.push_str(&summary);
            self.text.push('\n');
        }
        self.indent(depth);
        let start = self.text.len();
        self.text.push_str(&signature.text);

        let (line, end_line) = source_lines(self.source, statement);

        self.spans.push(LocatedSpan {
            span: formatter::RecordedSpan {
                start,
                end: self.text.len(),
                kind: signature.kind.to_string(),
                name: Some(signature.name),
            },
            line,
            end_line,
        });
        self.text.push('\n');
        self.write_expressions(statement, depth + 1);
        self.write_all(&container_members(statement), depth + 1);
    }

    /// Write the members a container declares as expressions.
    ///
    /// A struct's fields and an enum's variants have no signature of their own,
    /// so they reach an outline through neither `signature` nor `children`.
    /// They are also the whole of what those two declarations hold: an outline
    /// that skips them says nothing at all about a struct.
    fn write_expressions(&mut self, statement: &Statement, depth: usize) {
        for member in member_expressions(statement) {
            self.indent(depth);
            self.text.push_str(&formatter::expression_text(member));
            self.text.push('\n');
        }
    }

    /// Indent to `depth`, matching the formatter's indentation unit.
    fn indent(&mut self, depth: usize) {
        for _ in 0..depth {
            self.text.push_str("    ");
        }
    }
}

/// Whether a member is reachable from outside the type that declares it.
fn is_public(visibility: &MemberVisibility) -> bool {
    match visibility {
        MemberVisibility::Public => true,
        MemberVisibility::Private | MemberVisibility::Protected => false,
    }
}

/// Where a declaration's name sits in the source.
///
/// A declaration's own span opens at the keyword that introduces it, but a
/// reader looking for the declaration searches for its name, so the name's
/// position is the anchor. That name is on the declaration's first line, which
/// is what a comment above it attaches to.
fn declaration_offset(node: &Statement) -> Option<usize> {
    if let StatementKind::FunctionDeclaration(declaration) = &node.node {
        return Some(declaration.name_span.start);
    }
    if let Some(name) = resolve::container_name_expression(node) {
        return Some(name.span.start);
    }
    if let StatementKind::Use(path, _) = &node.node {
        return Some(path.span.start);
    }
    if let StatementKind::Type(declarations, _) = &node.node {
        return declarations.first().map(|entry| entry.span.start);
    }
    None
}

/// The declarations a container holds, or nothing for anything else.
fn container_members(node: &Statement) -> Vec<&Statement> {
    if resolve::container_name_expression(node).is_some() {
        return resolve::children(node);
    }
    Vec::new()
}

/// The members a container declares as expressions rather than statements.
///
/// A class keeps its fields in its body as statements, so they arrive through
/// `container_members`. A struct's fields and an enum's variants are parsed as
/// expressions and sit beside the methods, so they have to be asked for.
fn member_expressions(node: &Statement) -> &[crate::ast::expression::Expression] {
    match &node.node {
        StatementKind::Struct(_, _, fields, ..) => fields,
        StatementKind::Enum(_, _, variants, ..) => variants,
        StatementKind::Empty
        | StatementKind::Break
        | StatementKind::Continue
        | StatementKind::Expression(_)
        | StatementKind::Block(_)
        | StatementKind::Variable(..)
        | StatementKind::If(..)
        | StatementKind::While(..)
        | StatementKind::For(..)
        | StatementKind::Forall { .. }
        | StatementKind::GpuFrame(..)
        | StatementKind::GpuFrameBlock(_)
        | StatementKind::FunctionDeclaration(_)
        | StatementKind::Return(_)
        | StatementKind::Use(..)
        | StatementKind::Type(..)
        | StatementKind::Class(_)
        | StatementKind::Trait(..)
        | StatementKind::RuntimeFunctionDeclaration(..)
        | StatementKind::IntrinsicFunctionDeclaration(..) => &[],
    }
}

/// Build the report for a request that was answered.
fn success(
    shape: &Shape,
    rendered: LocatedRender,
    notes: Vec<Diagnostic>,
    source: &str,
    source_path: Option<String>,
) -> ViewReport {
    // A note is a warning: the read answered, so `ok` stays true, and the
    // warning rides along rather than being dropped for want of a place.
    let json = notes
        .iter()
        .map(|note| to_json(note, source, source_path.as_deref()))
        .collect::<Vec<JsonDiagnostic>>();
    let envelope = DiagnosticsEnvelope::new(JsonCommand::View, true, json).with_view(JsonView {
        shape: shape.label().to_string(),
        text: rendered.text.clone(),
        spans: rendered
            .spans
            .iter()
            .map(|located| JsonViewSpan {
                start: located.span.start,
                end: located.span.end,
                kind: located.span.kind.clone(),
                name: located.span.name.clone(),
                line: located.line,
                end_line: located.end_line,
            })
            .collect(),
    });

    ViewReport {
        envelope,
        ok: true,
        text: rendered.text,
        diagnostics: notes,
        source: source.to_string(),
        source_path,
    }
}

/// Build the report for a request that could not be answered.
fn failure(
    shape: &Shape,
    diagnostics: Vec<Diagnostic>,
    source: &str,
    source_path: Option<String>,
) -> ViewReport {
    let json = diagnostics
        .iter()
        .map(|diagnostic| to_json(diagnostic, source, source_path.as_deref()))
        .collect::<Vec<JsonDiagnostic>>();
    let _ = shape;

    ViewReport {
        envelope: DiagnosticsEnvelope::new(JsonCommand::View, false, json),
        ok: false,
        text: String::new(),
        diagnostics,
        source: source.to_string(),
        source_path,
    }
}

/// Report anchor text that the function does not contain.
fn anchor_not_found(anchor: &str) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldAnchorTextNotFound,
        format!(
            "`{}` does not occur in this function",
            sanitize_for_terminal(anchor)
        ),
        "the anchor is matched against canonical source, where comments and original spacing are normalized away",
    )
}

/// Report anchor text that occurs more than once.
fn anchor_not_unique(anchor: &str, count: usize) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldAnchorTextNotUnique,
        format!(
            "`{}` occurs {} times in this function",
            sanitize_for_terminal(anchor),
            count
        ),
        "extend the anchor until it matches one site only",
    )
}

/// Build a diagnostic carrying a registry code.
/// Report a file that could not be opened.
///
/// A caller that asked for JSON gets an envelope: answering a machine with a
/// bare line of prose would break the shape every other command promises.
fn report_unreadable(
    path: &Path,
    error: &std::io::Error,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    let diagnostic = coded(
        DiagnosticCode::BldInputNotReadable,
        format!("could not read {}: {}", path.display(), error),
        "check the path exists, names a file rather than a directory, and is readable",
    );

    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(
                JsonCommand::View,
                false,
                vec![to_json(&diagnostic, "", Some(&path.display().to_string()))],
            );
            println!("{}", serialize_envelope(&envelope));
        }
        Format::Pretty => eprint!(
            "{}",
            format_diagnostic_with_color(
                "",
                &diagnostic,
                Some(&path.display().to_string()),
                color_mode.into(),
            )
        ),
    }
    Outcome::Failed
}

/// Read part of `target` and write the result.
///
/// `target` is a file path or a module name; a missing one is a request that
/// only the shapes needing no source can answer, and those are dispatched
/// before this call.
pub fn run(target: Option<&str>, shape: &Shape, format: Format, color_mode: ColorMode) -> Outcome {
    let Some(target) = target else {
        return report(missing_target(shape), format, color_mode);
    };
    let path = match resolve_target(target) {
        Ok(path) => path,
        Err(diagnostic) => return report(*diagnostic, format, color_mode),
    };
    let path = path.as_path();
    let source = match std::fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => return report_unreadable(path, &error, format, color_mode),
    };

    let report = view(path, &source, shape);
    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => {
            if report.ok {
                print!("{}", report.text);
            }
            // A successful read can still carry a warning, and it goes to the
            // stream a warning goes to rather than into the text that was read.
            if !report.diagnostics.is_empty() {
                eprint!("{}", report.to_pretty(color_mode));
            }
        }
    }

    if report.ok {
        Outcome::Read
    } else {
        Outcome::Failed
    }
}

/// The members callable on `type_name`, one per line, own members first.
///
/// A member reached through `extends` or supplied by a trait carries the type
/// that declares it, so a reader can tell what is theirs to change from what
/// they inherited.
fn render_members(
    type_name: &str,
    definition: &crate::type_checker::context::TypeDefinition,
    definitions: &std::collections::HashMap<String, crate::type_checker::context::TypeDefinition>,
    public_only: bool,
) -> String {
    let mut out = String::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    out.push_str(&member_header(type_name, definition));
    out.push('\n');
    collect_members(
        type_name,
        type_name,
        definition,
        definitions,
        public_only,
        &mut seen,
        &mut out,
    );
    if seen.is_empty() {
        out.push_str("    (no members)\n");
    }
    out
}

/// The declaration line a member list sits under.
fn member_header(
    type_name: &str,
    definition: &crate::type_checker::context::TypeDefinition,
) -> String {
    use crate::type_checker::context::TypeDefinition;
    match definition {
        TypeDefinition::Class(class) => {
            let mut header = format!("class {}", type_name);
            if let Some(base) = &class.base_class {
                header.push_str(&format!(" extends {}", base));
            }
            if !class.traits.is_empty() {
                header.push_str(&format!(" implements {}", class.traits.join(", ")));
            }
            header
        }
        TypeDefinition::Struct(_) => format!("struct {}", type_name),
        TypeDefinition::Enum(_) => format!("enum {}", type_name),
        TypeDefinition::Trait(trait_definition) => {
            let mut header = format!("trait {}", type_name);
            if !trait_definition.parent_traits.is_empty() {
                header.push_str(&format!(
                    " extends {}",
                    trait_definition.parent_traits.join(", ")
                ));
            }
            header
        }
        TypeDefinition::Generic(_) => format!("type parameter {}", type_name),
        TypeDefinition::Alias(_) => format!("type {}", type_name),
    }
}

/// Append the members `definition` contributes, then those it inherits.
///
/// A name already written is not written again: an override is the member that
/// runs, so the nearest declaration is the one a caller reaches.
#[allow(clippy::too_many_arguments)]
fn collect_members(
    queried: &str,
    owner: &str,
    definition: &crate::type_checker::context::TypeDefinition,
    definitions: &std::collections::HashMap<String, crate::type_checker::context::TypeDefinition>,
    public_only: bool,
    seen: &mut std::collections::HashSet<String>,
    out: &mut String,
) {
    use crate::type_checker::context::TypeDefinition;
    match definition {
        TypeDefinition::Class(class) => {
            for (name, field) in &class.fields {
                if public_only && !matches!(field.visibility, MemberVisibility::Public) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push_str(&field_line(name, &field.ty, queried, owner));
                }
            }
            for (name, method) in &class.methods {
                if public_only && !matches!(method.visibility, MemberVisibility::Public) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push_str(&method_line(name, method, queried, owner));
                }
            }
            // Inherited members come after the type's own, and a trait's
            // defaults after both: that is the order a call resolves in.
            for source in class
                .base_class
                .iter()
                .map(String::as_str)
                .chain(class.traits.iter().map(String::as_str))
            {
                if let Some(next) = definitions.get(source) {
                    collect_members(queried, source, next, definitions, public_only, seen, out);
                }
            }
        }
        TypeDefinition::Struct(structure) => {
            for (name, ty, visibility) in &structure.fields {
                if public_only && !matches!(visibility, MemberVisibility::Public) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push_str(&field_line(name, ty, queried, owner));
                }
            }
        }
        TypeDefinition::Trait(trait_definition) => {
            for (name, method) in &trait_definition.methods {
                if public_only && !matches!(method.visibility, MemberVisibility::Public) {
                    continue;
                }
                if seen.insert(name.clone()) {
                    out.push_str(&method_line(name, method, queried, owner));
                }
            }
            for parent in &trait_definition.parent_traits {
                if let Some(next) = definitions.get(parent) {
                    collect_members(queried, parent, next, definitions, public_only, seen, out);
                }
            }
        }
        TypeDefinition::Enum(enumeration) => {
            for variant in enumeration.variants.keys() {
                if seen.insert(variant.clone()) {
                    out.push_str(&format!("    {}.{}\n", owner, variant));
                }
            }
        }
        TypeDefinition::Generic(_) | TypeDefinition::Alias(_) => {}
    }
}

/// One field, as a reader would write its type.
fn field_line(name: &str, ty: &crate::ast::types::Type, queried: &str, owner: &str) -> String {
    format!(
        "    {} {}{}\n",
        name,
        type_text(ty),
        declared_by(queried, owner)
    )
}

/// A type as Miri source spells it.
///
/// A type's `Display` is the diagnostic rendering — it writes a list as
/// `List(int)` so an error message reads well. This output is telling a reader
/// what to type, so it has to be the source form, `[int]`.
fn type_text(ty: &crate::ast::types::Type) -> String {
    let mut sink = crate::ast::formatter::sink::Sink::new();
    crate::ast::formatter::types::type_kind(&mut sink, &ty.kind);
    sink.text().to_string()
}

/// One method, as a reader would write its signature.
fn method_line(
    name: &str,
    method: &crate::type_checker::context::MethodInfo,
    queried: &str,
    owner: &str,
) -> String {
    let params = method
        .params
        .iter()
        .enumerate()
        .map(|(index, (param_name, param_type))| {
            let out_marker = if method.is_param_out(index) {
                "out "
            } else {
                ""
            };
            format!("{}{} {}", out_marker, param_name, type_text(param_type))
        })
        .collect::<Vec<_>>()
        .join(", ");
    let returns = match &method.return_type.kind {
        crate::ast::types::TypeKind::Void => String::new(),
        _ => format!(" {}", type_text(&method.return_type)),
    };
    let prefix = if method.is_static { "static fn" } else { "fn" };
    format!(
        "    {} {}({}){}{}\n",
        prefix,
        name,
        params,
        returns,
        declared_by(queried, owner)
    )
}

/// The `from` note an inherited member carries.
///
/// A member the queried type declares itself needs no note: naming the type a
/// reader already asked about would be noise on every line.
fn declared_by(queried: &str, owner: &str) -> String {
    if queried == owner {
        return String::new();
    }
    format!("    // from {}", owner)
}

/// Every member callable on `type_name`, its own and those it inherits.
///
/// This runs the frontend rather than reading the file, because inheritance and
/// trait membership are properties the parser does not know: a method reached
/// through `extends`, or supplied by a trait, is declared somewhere else and
/// often in another module. Only the type table has followed those links.
///
/// A `None` path answers from the prelude alone, which is what lets a caller ask
/// about a library type without first knowing which module declares it.
pub fn members(
    path: Option<&Path>,
    source: &str,
    type_name: &str,
    public_only: bool,
) -> ViewReport {
    let shape = Shape::Members {
        type_name: type_name.to_string(),
        public_only,
    };
    let source_path = path.map(|path| path.display().to_string());
    let pipeline = crate::cli::anchor::pipeline_for(path);
    let result = match pipeline.frontend(source) {
        Ok(result) => result,
        // The table is built by the pass that reports these, so a program the
        // frontend rejected has no answer to give. Reporting the errors is more
        // use than a list assembled from a half-built table.
        Err(error) => return failure(&shape, error.to_diagnostics(), source, source_path),
    };

    let definitions = &result.type_checker.type_table.global_type_definitions;
    let Some(definition) = definitions.get(type_name) else {
        return failure(
            &shape,
            vec![*type_not_found(type_name, definitions)],
            source,
            source_path,
        );
    };

    let text = render_members(type_name, definition, definitions, public_only);
    success(
        &shape,
        LocatedRender {
            text,
            spans: Vec::new(),
        },
        Vec::new(),
        source,
        source_path,
    )
}

/// Report a type the program does not have in scope, naming the nearest it has.
fn type_not_found(
    type_name: &str,
    definitions: &std::collections::HashMap<String, crate::type_checker::context::TypeDefinition>,
) -> Box<Diagnostic> {
    let mut names: Vec<&str> = definitions.keys().map(String::as_str).collect();
    names.sort_unstable();
    let help = match crate::error::format::find_best_match(type_name, &names) {
        Some(nearest) => format!("did you mean '{}'?", nearest),
        None => "run `miri view <MODULE> --outline --public` to list what a module declares."
            .to_string(),
    };
    coded(
        DiagnosticCode::BldTypeNotInScope,
        format!(
            "no type named '{}' is in scope",
            sanitize_for_terminal(type_name)
        ),
        &help,
    )
}

/// The file a `view` argument names: a path on disk, or a module resolved the
/// way an `use` statement resolves it.
///
/// A file is tried first, so an argument that names one is never reinterpreted.
/// Module resolution runs through the compiler's own search, so `view` and the
/// type checker can never disagree about which file a module name refers to.
pub fn resolve_target(argument: &str) -> Result<std::path::PathBuf, Box<Diagnostic>> {
    let as_path = std::path::PathBuf::from(argument);
    if as_path.is_file() {
        return Ok(as_path);
    }
    if let Some(found) = crate::type_checker::statements::imports::locate_module(argument, None) {
        return Ok(found);
    }
    Err(target_not_found(argument))
}

/// Report an argument that names neither a readable file nor a known module.
fn target_not_found(argument: &str) -> Box<Diagnostic> {
    let roots = crate::type_checker::statements::imports::stdlib_roots()
        .iter()
        .map(|root| format!("  - {}", root.display()))
        .collect::<Vec<_>>()
        .join("\n");
    coded(
        DiagnosticCode::BldInputNotReadable,
        format!(
            "could not read {}: no such file, and no module of that name",
            sanitize_for_terminal(argument)
        ),
        &format!(
            "a module name is searched in these roots, highest priority first:\n{}\n\
             set MIRI_STDLIB_PATH to search somewhere else.",
            roots
        ),
    )
}

/// The stdlib search roots, in the order a module name is looked for, each
/// marked with whether it is present on this machine.
///
/// The binary knows where it looked; until it says so, a caller whose module
/// did not resolve has nothing to check.
pub fn stdlib_roots_text() -> String {
    crate::type_checker::statements::imports::stdlib_roots()
        .iter()
        .map(|root| {
            let state = if root.is_dir() { "present" } else { "absent" };
            format!("{}\t{}\n", state, root.display())
        })
        .collect()
}

/// Report a shape that needs a file when none was given.
fn missing_target(shape: &Shape) -> Diagnostic {
    *coded(
        DiagnosticCode::BldInputNotReadable,
        format!("`--{}` needs a file or module to read", shape.label()),
        "give a path such as `main.mi`, or a module name such as `system.string`.",
    )
}

/// Write one diagnostic in the requested form and report the command failed.
fn report(diagnostic: Diagnostic, format: Format, color_mode: ColorMode) -> Outcome {
    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(
                JsonCommand::View,
                false,
                vec![to_json(&diagnostic, "", None)],
            )
            .with_exit_code(1);
            println!("{}", serialize_envelope(&envelope));
        }
        Format::Pretty => eprint!(
            "{}",
            format_diagnostic_with_color("", &diagnostic, None, color_mode.into())
        ),
    }
    Outcome::Failed
}

/// Answer `--type` for a program, or for the prelude when no path is given.
pub fn run_members(
    target: Option<&str>,
    type_name: &str,
    public_only: bool,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    let resolved = match target {
        Some(target) => match resolve_target(target) {
            Ok(path) => Some(path),
            Err(diagnostic) => return report(*diagnostic, format, color_mode),
        },
        None => None,
    };
    // With no file to read, the question is answered against an empty program,
    // whose scope is the implicit prelude. That is what lets a caller ask about
    // a library type before knowing which module declares it.
    let source = match &resolved {
        Some(path) => match std::fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => return report_unreadable(path, &error, format, color_mode),
        },
        None => String::new(),
    };

    let report = members(resolved.as_deref(), &source, type_name, public_only);
    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => {
            if report.ok {
                print!("{}", report.text);
            } else {
                eprint!("{}", report.to_pretty(color_mode));
            }
        }
    }

    if report.ok {
        Outcome::Read
    } else {
        Outcome::Failed
    }
}

/// Print the roots a module name is searched in.
pub fn run_stdlib_roots(format: Format) -> Outcome {
    match format {
        Format::Json => {
            let envelope = DiagnosticsEnvelope::new(JsonCommand::View, true, vec![])
                .with_exit_code(0)
                .with_view(JsonView {
                    shape: "stdlib-root".to_string(),
                    text: stdlib_roots_text(),
                    spans: Vec::new(),
                });
            println!("{}", serialize_envelope(&envelope));
        }
        Format::Pretty => print!("{}", stdlib_roots_text()),
    }
    Outcome::Read
}
