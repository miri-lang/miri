// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Finding `.mi` files that declare `@test` functions, and refusing the ones
//! that cannot be run as test files.
//!
//! Discovery parses but does not type-check: a parse is cheap and needs no
//! stdlib resolution, and the attribute markers are all it has to read.

use serde::Serialize;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

use crate::ast::{Statement, StatementKind, IGNORE_ATTRIBUTE, TEST_ATTRIBUTE, XFAIL_ATTRIBUTE};
use crate::lexer::Lexer;
use crate::parser::Parser;
use crate::pipeline::is_script_body_statement;
use crate::test_runner::{display_path, TestMarker};

/// A file that declares at least one `@test` function.
#[derive(Debug)]
pub struct TestFile {
    pub path: PathBuf,
    pub source: String,
    pub tests: Vec<TestMarker>,
}

/// Why a file holding `@test` functions cannot be run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RejectionReason {
    /// Contains `@test` but does not parse, so its tests cannot be collected.
    /// Skipping it silently would report a typo'd test file as "0 tests, ok".
    Unparseable,
    /// Declares its own `main`, which would collide with the dispatcher the
    /// runner appends.
    DeclaresMain,
    /// Holds executable statements outside any function. Script-mode wrapping
    /// is skipped once the appended dispatcher declares `main`, so those
    /// statements would be dropped without a word.
    TopLevelStatements,
}

impl std::fmt::Display for RejectionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let explanation = match self {
            RejectionReason::Unparseable => {
                "declares `@test` but does not parse; run `miri check` on it for the syntax error"
            }
            RejectionReason::DeclaresMain => {
                "declares its own `main`; a test file holds only declarations, and the runner supplies the entry point"
            }
            RejectionReason::TopLevelStatements => {
                "has executable statements outside a function; move them into a `@test` function, where they would otherwise be silently skipped"
            }
        };
        write!(f, "{}", explanation)
    }
}

/// A file the runner refused, and why.
#[derive(Debug, Clone, Serialize)]
pub struct RejectedFile {
    pub path: String,
    pub reason: RejectionReason,
}

/// Everything a directory walk turned up.
#[derive(Debug, Default)]
pub struct Discovered {
    pub files: Vec<TestFile>,
    pub rejected: Vec<RejectedFile>,
}

/// The directory a run's results are named relative to.
///
/// A run pointed at one file still names it the way a walk of its directory
/// would, so the same test reads as the same string whichever way it was asked
/// for and a filter written against one form matches the other.
pub fn root_of(target: &Path) -> PathBuf {
    if target.is_file() {
        return target
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| PathBuf::from("."));
    }
    target.to_path_buf()
}

/// Find the `.mi` files declaring `@test` functions that `target` names.
///
/// `target` is either one file or a directory to walk. Naming a file runs that
/// file and nothing else: a sibling is neither compiled nor able to reject the
/// run, which is what a caller asked for by naming one file rather than the
/// directory holding it. Selecting the file this way rather than by matching
/// its name keeps a sibling whose name contains it from being swept in.
///
/// A file that neither parses nor mentions `@test` is skipped in silence: it is
/// simply not a test file, and `miri check` is the tool for its syntax errors.
pub fn discover(target: &Path) -> std::io::Result<Discovered> {
    let mut discovered = Discovered::default();
    let root = root_of(target);

    if target.is_file() {
        let source = std::fs::read_to_string(target)?;
        classify(target, source, &root, &mut discovered);
        return Ok(discovered);
    }

    for entry in WalkDir::new(target).into_iter().filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().is_none_or(|ext| ext != "mi") {
            continue;
        }
        let Ok(source) = std::fs::read_to_string(path) else {
            continue;
        };
        classify(path, source, &root, &mut discovered);
    }

    Ok(discovered)
}

/// Sort one file into a runnable test file, a rejection, or neither.
fn classify(path: &Path, source: String, dir: &Path, discovered: &mut Discovered) {
    let display = display_path(path, dir);

    let Some(program_body) = parse_body(&source) else {
        if mentions_test_attribute(&source) {
            discovered.rejected.push(RejectedFile {
                path: display,
                reason: RejectionReason::Unparseable,
            });
        }
        return;
    };

    let tests = collect_markers(&program_body);
    if tests.is_empty() {
        return;
    }

    if let Some(reason) = rejection_reason(&program_body) {
        discovered.rejected.push(RejectedFile {
            path: display,
            reason,
        });
        return;
    }

    discovered.files.push(TestFile {
        path: path.to_path_buf(),
        source,
        tests,
    });
}

/// Parse to a statement list, or `None` when the source does not parse.
fn parse_body(source: &str) -> Option<Vec<Statement>> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    parser.parse().ok().map(|program| program.body)
}

/// A lexical check, deliberately: the file did not parse, so there is no AST to
/// ask whether the author meant it as a test file.
fn mentions_test_attribute(source: &str) -> bool {
    source.contains("@test")
}

/// The reason this file cannot host a synthesized dispatcher, if any.
fn rejection_reason(body: &[Statement]) -> Option<RejectionReason> {
    if body.iter().any(declares_main) {
        return Some(RejectionReason::DeclaresMain);
    }
    if body.iter().any(is_script_body_statement) {
        return Some(RejectionReason::TopLevelStatements);
    }
    None
}

fn declares_main(statement: &Statement) -> bool {
    match &statement.node {
        StatementKind::FunctionDeclaration(declaration) => declaration.name == "main",
        _ => false,
    }
}

/// Every `@test` function in the file, in declaration order.
fn collect_markers(body: &[Statement]) -> Vec<TestMarker> {
    body.iter().filter_map(marker_of).collect()
}

fn marker_of(statement: &Statement) -> Option<TestMarker> {
    let StatementKind::FunctionDeclaration(declaration) = &statement.node else {
        return None;
    };
    let attributes = &declaration.attributes;
    if !attributes.iter().any(|a| a.name == TEST_ATTRIBUTE) {
        return None;
    }

    Some(TestMarker {
        name: declaration.name.clone(),
        ignore_reason: argument_of(attributes, IGNORE_ATTRIBUTE),
        xfail_reason: argument_of(attributes, XFAIL_ATTRIBUTE),
    })
}

fn argument_of(attributes: &[crate::ast::Attribute], name: &str) -> Option<String> {
    attributes
        .iter()
        .find(|a| a.name == name)
        .and_then(|a| a.argument.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body_of(source: &str) -> Vec<Statement> {
        parse_body(source).expect("source should parse")
    }

    #[test]
    fn collects_a_plain_test() {
        let markers = collect_markers(&body_of("@test\nfn test_adds()\n    var x = 1\n"));
        assert_eq!(markers.len(), 1);
        assert_eq!(markers[0].name, "test_adds");
        assert!(!markers[0].is_ignored());
        assert!(!markers[0].is_xfail());
    }

    #[test]
    fn reads_the_ignore_reason() {
        let markers = collect_markers(&body_of(
            "@test\n@ignore(\"flaky on CI\")\nfn test_skipped()\n    var x = 1\n",
        ));
        assert!(markers[0].is_ignored());
        assert_eq!(markers[0].ignore_reason.as_deref(), Some("flaky on CI"));
    }

    #[test]
    fn reads_the_xfail_reason() {
        let markers = collect_markers(&body_of(
            "@test\n@xfail(\"known bug\")\nfn test_broken()\n    var x = 1\n",
        ));
        assert!(markers[0].is_xfail());
        assert_eq!(markers[0].xfail_reason.as_deref(), Some("known bug"));
    }

    #[test]
    fn keeps_declaration_order() {
        let markers = collect_markers(&body_of(
            "@test\nfn test_b()\n    var x = 1\n\n@test\nfn test_a()\n    var y = 2\n",
        ));
        let names: Vec<&str> = markers.iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, vec!["test_b", "test_a"]);
    }

    #[test]
    fn ignores_functions_without_the_marker() {
        let markers = collect_markers(&body_of("fn helper()\n    var x = 1\n"));
        assert!(markers.is_empty());
    }

    #[test]
    fn rejects_a_file_declaring_main() {
        let body = body_of("@test\nfn test_a()\n    var x = 1\n\nfn main() int\n    return 0\n");
        assert_eq!(rejection_reason(&body), Some(RejectionReason::DeclaresMain));
    }

    #[test]
    fn rejects_a_file_with_top_level_statements() {
        let body = body_of("var top = 1\n\n@test\nfn test_a()\n    var x = 1\n");
        assert_eq!(
            rejection_reason(&body),
            Some(RejectionReason::TopLevelStatements)
        );
    }

    #[test]
    fn accepts_a_file_of_declarations_only() {
        let body = body_of("const LIMIT = 3\n\n@test\nfn test_a()\n    var x = LIMIT\n");
        assert_eq!(rejection_reason(&body), None);
    }

    #[test]
    fn an_unparseable_file_is_only_flagged_when_it_mentions_the_marker() {
        assert!(parse_body("@test\nfn test_broken(\n").is_none());
        assert!(mentions_test_attribute("@test\nfn test_broken(\n"));
        assert!(!mentions_test_attribute("fn broken(\n"));
    }
}
