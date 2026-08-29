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

use crate::ast::doc_comments::DocComments;
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
    Outline,
}

impl Shape {
    /// The name this shape carries in the JSON envelope.
    fn label(&self) -> &'static str {
        match self {
            Shape::Function {
                around: Some(_), ..
            } => "around",
            Shape::Function { around: None, .. } => "fn",
            Shape::Outline => "outline",
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
        Shape::Outline => Ok(outline(&program, source)),
        Shape::Function { name, around } => function_view(&program, name, around.as_deref()),
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
    name: &str,
    anchor: Option<&str>,
) -> Result<Rendered, Box<Diagnostic>> {
    let declaration = resolve::resolve(program, name)?;
    let rendered = formatter::declaration(declaration);
    let Some(anchor) = anchor else {
        return Ok(rendered);
    };
    narrow(declaration, &rendered, anchor)
}

/// Narrow a rendered function to the innermost block holding `anchor`.
fn narrow(
    declaration: &Statement,
    rendered: &Rendered,
    anchor: &str,
) -> Result<Rendered, Box<Diagnostic>> {
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
        .map(formatter::declaration)
        .find(|block| block.text.contains(anchor));

    // An anchor that matches only the signature belongs to no block; the
    // function itself is then the narrowest honest answer.
    Ok(innermost.unwrap_or_else(|| rendered.clone()))
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
fn outline(program: &Program, source: &str) -> Rendered {
    let comments = DocComments::harvest(source);
    let mut outline = Outline {
        comments,
        source,
        text: String::new(),
        spans: Vec::new(),
    };
    outline.write_all(&program.body.iter().collect::<Vec<_>>(), 0);
    Rendered {
        text: outline.text,
        spans: outline.spans,
    }
}

/// Accumulates an outline as it walks a program's declarations.
struct Outline<'a> {
    comments: DocComments,
    source: &'a str,
    text: String,
    spans: Vec<formatter::RecordedSpan>,
}

impl Outline<'_> {
    /// Write every declaration among `statements`, and their members.
    fn write_all(&mut self, statements: &[&Statement], depth: usize) {
        for statement in statements {
            self.write_one(statement, depth);
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
        self.spans.push(formatter::RecordedSpan {
            start,
            end: self.text.len(),
            kind: signature.kind.to_string(),
            name: Some(signature.name),
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
    rendered: Rendered,
    source: &str,
    source_path: Option<String>,
) -> ViewReport {
    let envelope = DiagnosticsEnvelope::new(JsonCommand::View, true, vec![]).with_view(JsonView {
        shape: shape.label().to_string(),
        text: rendered.text.clone(),
        spans: rendered
            .spans
            .iter()
            .map(|span| JsonViewSpan {
                start: span.start,
                end: span.end,
                kind: span.kind.clone(),
                name: span.name.clone(),
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
