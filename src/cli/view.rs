// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri view` command: read part of a program instead of all of it.
//!
//! The command is split the way [`crate::cli::check`] is: [`view`] does the work
//! and returns what it found without printing, so a long-lived server can call
//! it, and [`run`] adds the writing for the command line.
//!
//! Everything is rendered from the parsed AST rather than sliced out of the
//! file, so the text a tool reads here is the same text it reads next time and
//! the spans that come with it index that text. The source is parsed exactly as
//! written — no script-mode wrapping and no type normalization — so an outline
//! never lists a `main` the author did not write, and a type written `[int]`
//! reads back as `[int]`.

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
    },
    /// Every declaration's signature, with no bodies.
    Outline { public_only: bool },
}

impl Shape {
    /// The name this shape carries in the JSON envelope.
    fn label(&self) -> &'static str {
        match self {
            Shape::Function {
                around: Some(_), ..
            } => "around",
            Shape::Function { around: None, .. } => "fn",
            Shape::Outline { .. } => "outline",
        }
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
    let program = match parse(source) {
        Ok(program) => program,
        Err(diagnostic) => return failure(shape, vec![*diagnostic], source, source_path),
    };

    let rendered = match shape {
        Shape::Outline { public_only } => Ok(outline(&program, source, *public_only)),
        Shape::Function { name, around } => {
            function_view(&program, source, name, around.as_deref())
        }
    };

    match rendered {
        Ok(rendered) => success(shape, rendered, source, source_path),
        Err(diagnostic) => failure(shape, vec![*diagnostic], source, source_path),
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
) -> Result<LocatedRender, Box<Diagnostic>> {
    let declaration = resolve::resolve(program, name)?;
    let rendered = formatter::declaration(declaration);
    let Some(anchor) = anchor else {
        return Ok(located_declaration(source, declaration, rendered));
    };
    let (narrowed, node) = narrow(declaration, &rendered, anchor)?;
    Ok(located_block(source, node, narrowed))
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

/// Narrow a rendered function to the innermost block holding `anchor`.
///
/// Returns the rendering together with the statement it came from, so the
/// caller can report where in the file that block lives.
fn narrow<'a>(
    declaration: &'a Statement,
    rendered: &Rendered,
    anchor: &str,
) -> Result<(Rendered, &'a Statement), Box<Diagnostic>> {
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
        .map(|block| (formatter::declaration(block), block))
        .find(|(text, _)| text.text.contains(anchor));

    // An anchor that matches only the signature belongs to no block; the
    // function itself is then the narrowest honest answer.
    Ok(innermost.unwrap_or_else(|| (rendered.clone(), declaration)))
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
        self.write_all(&container_members(statement), depth + 1);
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
/// A declaration statement is built by the AST factory, which has no source
/// text and so leaves the statement's own span empty; the name carries the real
/// position. That name is on the declaration's first line, which is what a
/// comment above it attaches to.
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

/// Build the report for a request that was answered.
fn success(
    shape: &Shape,
    rendered: LocatedRender,
    source: &str,
    source_path: Option<String>,
) -> ViewReport {
    let envelope = DiagnosticsEnvelope::new(JsonCommand::View, true, vec![]).with_view(JsonView {
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
        diagnostics: vec![],
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

/// Read part of `path` and write the result.
pub fn run(path: &Path, shape: &Shape, format: Format, color_mode: ColorMode) -> Outcome {
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
