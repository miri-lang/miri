// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! PEG grammar interpreter for validating Miri's token-level grammar.

pub mod parser;

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use miri::lexer::terminal::{classify_token, published_terminals, TerminalClassification};
use miri::lexer::{Lexer, Token};
use miri::parser::Parser;

use self::parser::{PegExpr, PegGrammar, PegMatcher};

/// Number of corpus files the accept gate must exercise. Pinned so a file that
/// disappears from the walk fails the gate instead of quietly reducing coverage.
const ACCEPT_CORPUS_SIZE: usize = 86;

/// Number of fixtures the reject gate must exercise, pinned for the same reason.
const REJECT_CORPUS_SIZE: usize = 22;

/// Files that are not accepted into the grammar corpus and why.
fn exclusion_table() -> HashMap<String, String> {
    let mut m = HashMap::new();
    m.insert(
        "tests/e2e/err_03_indentation.mi".to_string(),
        "lexer rejects (IndentationMismatch) before parser runs".to_string(),
    );
    m
}

/// Discovers all .mi files under a corpus root.
fn discover_mi_files(root: &Path) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();
    let entries = fs::read_dir(root)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            match discover_mi_files(&path) {
                Ok(mut subfiles) => files.append(&mut subfiles),
                Err(e) => return Err(e),
            }
        } else if path.extension().map(|e| e == "mi").unwrap_or(false) {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

/// Renders a PEG match failure as the token it stopped on plus a little context,
/// Renders the farthest point a match reached as the token it stopped on plus a
/// little context. PEG backtracking reports failure at the start of the construct
/// it abandoned, which is rarely where the grammar is wrong; the farthest position
/// reached across all alternatives is.
fn describe_peg_failure(pos: usize, tokens: &[Token]) -> String {
    let name = |i: usize| match tokens.get(i).map(classify_token) {
        Some(TerminalClassification::Terminal(n)) => n,
        Some(TerminalClassification::NotTerminal(r)) => format!("<non-terminal: {r}>"),
        None => "<end of input>".to_string(),
    };
    let start = pos.saturating_sub(6);
    let before: Vec<String> = (start..pos).map(name).collect();
    let after: Vec<String> = (pos + 1..(pos + 4).min(tokens.len())).map(name).collect();
    format!(
        "farthest token {} of {}: {} <<{}>> {}",
        pos,
        tokens.len(),
        before.join(" "),
        name(pos),
        after.join(" "),
    )
}

/// A stable digest of the grammar text, used to detect an edit that skipped the
/// changelog. Line endings are normalised so a checkout's newline style cannot
/// change the value.
fn grammar_digest(text: &str) -> String {
    let normalised: String = text.replace("\r\n", "\n");
    let mut hash: u128 = 0xcbf2_9ce4_8422_2325;
    for byte in normalised.as_bytes() {
        hash ^= u128::from(*byte);
        hash = hash.wrapping_mul(0x1000_0000_01b3);
    }
    format!("{hash:032x}")
}

/// Collects every name a rule body references, whether written bare or quoted.
fn collect_referenced_names(expr: &PegExpr, out: &mut Vec<String>) {
    match expr {
        PegExpr::RuleRef(name) | PegExpr::Literal(name) => out.push(name.clone()),
        PegExpr::Sequence(items) | PegExpr::Choice(items) => {
            for item in items {
                collect_referenced_names(item, out);
            }
        }
        PegExpr::Star(inner)
        | PegExpr::Plus(inner)
        | PegExpr::Optional(inner)
        | PegExpr::And(inner)
        | PegExpr::Not(inner)
        | PegExpr::Group(inner) => collect_referenced_names(inner, out),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_grammar_file_exists() {
        let grammar_path = Path::new("docs/grammar.peg");
        assert!(
            grammar_path.exists(),
            "docs/grammar.peg must exist for grammar validation"
        );
    }

    #[test]
    fn test_grammar_version_and_changelog_agree() {
        let grammar_text =
            fs::read_to_string("docs/grammar.peg").expect("should read docs/grammar.peg");
        let changelog = fs::read_to_string("docs/grammar-changelog.md")
            .expect("should read docs/grammar-changelog.md");

        let versions: Vec<&str> = grammar_text
            .lines()
            .filter_map(|line| line.strip_prefix("# version:"))
            .map(str::trim)
            .collect();
        assert_eq!(
            versions.len(),
            1,
            "the grammar must carry exactly one `# version:` line, found {}",
            versions.len()
        );
        let version = versions[0];

        assert!(
            changelog.contains(&format!("## Version {version}")),
            "the changelog has no entry for grammar version {version}"
        );

        // The recorded hash is what forces a grammar edit through the changelog: any
        // change to the rules invalidates it until the author either bumps the version
        // with a new entry or restates the hash as a deliberate non-breaking change.
        let recorded = changelog
            .lines()
            .find_map(|line| line.split("**Content Hash**: `").nth(1))
            .and_then(|rest| rest.split('`').next())
            .expect("the changelog entry must record a content hash");
        assert_eq!(
            recorded,
            grammar_digest(&grammar_text),
            "docs/grammar.peg changed but docs/grammar-changelog.md was not updated"
        );
    }

    #[test]
    fn test_grammar_changelog_exists() {
        let changelog_path = Path::new("docs/grammar-changelog.md");
        assert!(
            changelog_path.exists(),
            "docs/grammar-changelog.md must exist"
        );
    }

    #[test]
    fn test_corpus_completeness() {
        let mut discovered = Vec::new();
        let exclusions = exclusion_table();

        let corpus_roots = ["examples", "tests/e2e", "tests/stdlib", "src/stdlib"];
        for root_str in &corpus_roots {
            let root_path = Path::new(root_str);
            if !root_path.exists() {
                panic!(
                    "corpus root '{}' does not exist; gate cannot proceed",
                    root_str
                );
            }
            let files = discover_mi_files(root_path).unwrap_or_else(|e| {
                panic!(
                    "cannot read corpus root '{}': {}; gate cannot proceed",
                    root_str, e
                )
            });

            for file in files {
                let rel_path = file
                    .strip_prefix(".")
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .to_string();
                discovered.push(rel_path);
            }
        }

        let accepted_count = discovered
            .iter()
            .filter(|f| !exclusions.contains_key(*f))
            .count();

        assert_eq!(
            accepted_count, ACCEPT_CORPUS_SIZE,
            "accept corpus should hold every discovered file that is not excluded"
        );

        // An exclusion naming a file that no longer exists hides a gap rather than
        // recording one, so a stale entry fails the gate as loudly as a missing one.
        for excluded in exclusions.keys() {
            assert!(
                discovered.iter().any(|found| found == excluded),
                "exclusion table names `{excluded}`, which the corpus walk did not find"
            );
        }
    }

    #[test]
    fn test_reject_corpus_lexes_cleanly() {
        let reject_dir = Path::new("tests/fixtures/grammar/reject");
        assert!(
            reject_dir.exists(),
            "the reject corpus is what proves the grammar discriminates; it must exist"
        );

        let entries = fs::read_dir(reject_dir).expect("should read reject dir");
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.extension().map(|e| e == "mi").unwrap_or(false) {
                continue;
            }

            let source = fs::read_to_string(&path)
                .unwrap_or_else(|_| panic!("should read {}", path.display()));

            let mut lexer = Lexer::new(&source);
            for result in &mut lexer {
                if let Err(e) = result {
                    panic!(
                        "reject fixture {} failed to lex cleanly: {}",
                        path.display(),
                        e
                    );
                }
            }
        }
    }

    #[test]
    fn test_grammar_terminals_resolve() {
        let grammar_text =
            fs::read_to_string("docs/grammar.peg").expect("should read docs/grammar.peg");
        let grammar =
            PegGrammar::parse(&grammar_text).expect("grammar.peg should parse successfully");

        let published: HashSet<String> = published_terminals().into_iter().collect();

        let mut unknown: Vec<String> = Vec::new();
        for (rule_name, expr) in &grammar.rules {
            let mut referenced = Vec::new();
            collect_referenced_names(expr, &mut referenced);
            for name in referenced {
                if grammar.rules.contains_key(&name) || published.contains(&name) {
                    continue;
                }
                unknown.push(format!("rule `{rule_name}` references `{name}`"));
            }
        }
        unknown.sort();
        unknown.dedup();

        assert!(
            unknown.is_empty(),
            "grammar references names that are neither a rule nor a terminal the lexer \
             can produce, so they can never match:\n  {}",
            unknown.join("\n  ")
        );
    }

    #[test]
    fn test_differential_gate_accept_corpus() {
        let grammar_text =
            fs::read_to_string("docs/grammar.peg").expect("should read docs/grammar.peg");
        let grammar =
            PegGrammar::parse(&grammar_text).expect("grammar.peg should parse successfully");

        let mut discovered = Vec::new();
        let exclusions = exclusion_table();

        let corpus_roots = ["examples", "tests/e2e", "tests/stdlib", "src/stdlib"];
        for root_str in &corpus_roots {
            let root_path = Path::new(root_str);
            assert!(
                root_path.exists(),
                "corpus root '{root_str}' does not exist; the gate would silently \
                 cover fewer files than it claims"
            );
            let files = discover_mi_files(root_path)
                .unwrap_or_else(|e| panic!("cannot read corpus root '{root_str}': {e}"));
            for file in files {
                let rel_path = file
                    .strip_prefix(".")
                    .unwrap_or(&file)
                    .to_string_lossy()
                    .to_string();
                discovered.push((rel_path, file));
            }
        }

        let mut mismatch_count = 0;
        let mut tested_count = 0;

        for (rel_path, file_path) in discovered {
            if exclusions.contains_key(&rel_path) {
                continue;
            }

            tested_count += 1;
            let source = fs::read_to_string(&file_path).expect("should read accept corpus file");

            let real_parse_ok = {
                let mut lexer = Lexer::new(&source);
                let mut parser = Parser::new(&mut lexer, &source);
                parser.parse().is_ok()
            };

            let mut tokens = Vec::new();
            let mut lex_error = None;
            {
                let mut lexer = Lexer::new(&source);
                for result in &mut lexer {
                    match result {
                        Ok((token, _)) => tokens.push(token),
                        Err(e) => {
                            lex_error = Some(e);
                            break;
                        }
                    }
                }
            }

            // A corpus file that does not lex cannot exercise the grammar. That is a
            // gate failure, not a reason to pass: it means the file belongs in the
            // exclusion table with a stated reason.
            if let Some(e) = lex_error {
                mismatch_count += 1;
                eprintln!("  LEX-FAILED {rel_path} - {e:?} (needs an exclusion entry)");
                continue;
            }

            let mut matcher = PegMatcher::new(grammar.clone());
            let peg_result = matcher.match_tokens(&tokens);
            let peg_match_ok = peg_result.is_ok();

            if real_parse_ok != peg_match_ok {
                mismatch_count += 1;
                eprintln!(
                    "  MISMATCH {rel_path} - real_parser: {real_parse_ok}, peg: {peg_match_ok}"
                );
                if peg_result.is_err() {
                    eprintln!(
                        "      {}",
                        describe_peg_failure(matcher.farthest(), &tokens)
                    );
                }
            }
        }

        assert_eq!(
            tested_count, ACCEPT_CORPUS_SIZE,
            "the differential gate must test every file in the accept corpus"
        );
        assert_eq!(
            mismatch_count, 0,
            "differential gate found {} mismatches between real parser and PEG matcher",
            mismatch_count
        );
    }

    #[test]
    fn test_differential_gate_reject_corpus() {
        let grammar_text =
            fs::read_to_string("docs/grammar.peg").expect("should read docs/grammar.peg");
        let grammar =
            PegGrammar::parse(&grammar_text).expect("grammar.peg should parse successfully");

        let reject_dir = Path::new("tests/fixtures/grammar/reject");
        assert!(
            reject_dir.exists(),
            "the reject corpus is what proves the grammar discriminates; it must exist"
        );

        let entries = fs::read_dir(reject_dir).expect("should read reject dir");
        let mut files: Vec<_> = entries.flatten().collect();
        files.sort_by_key(|e| e.path());

        let mut mismatch_count = 0;
        let mut tested_count = 0;

        for entry in files {
            let path = entry.path();
            if !path.extension().map(|e| e == "mi").unwrap_or(false) {
                continue;
            }

            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            let source = fs::read_to_string(&path).expect("should read reject fixture");

            let mut lexer = Lexer::new(&source);
            let mut tokens = Vec::new();
            let mut lex_error = None;
            for result in &mut lexer {
                match result {
                    Ok((token, _)) => tokens.push(token),
                    Err(e) => {
                        lex_error = Some(e);
                        break;
                    }
                }
            }

            // A fixture that dies in the lexer never reaches the grammar, so it
            // proves nothing about it. Skipping one would let the corpus quietly
            // shrink to nothing; it has to be rewritten instead.
            assert!(
                lex_error.is_none(),
                "reject fixture {file_name} does not lex, so it cannot exercise the \
                 grammar: {lex_error:?}"
            );

            tested_count += 1;

            let real_parse_ok = {
                let mut lexer = Lexer::new(&source);
                let mut parser = Parser::new(&mut lexer, &source);
                parser.parse().is_ok()
            };

            let peg_match_ok = {
                let mut matcher = PegMatcher::new(grammar.clone());
                matcher.match_tokens(&tokens).is_ok()
            };

            // A fixture the real parser accepts is not a reject fixture at all. Both
            // parsers would agree on it and the gate would pass while proving nothing,
            // so the corpus itself is checked, not just the agreement.
            assert!(
                !real_parse_ok,
                "reject fixture {file_name} parses cleanly; it does not test rejection"
            );

            if real_parse_ok != peg_match_ok {
                mismatch_count += 1;
                eprintln!(
                    "  MISMATCH {} - real_parser: {}, peg: {}",
                    file_name, real_parse_ok, peg_match_ok
                );
            }
        }

        assert_eq!(
            tested_count, REJECT_CORPUS_SIZE,
            "the reject gate must test every fixture in the reject corpus"
        );
        assert_eq!(
            mismatch_count, 0,
            "reject gate found {} mismatches between real parser and PEG matcher",
            mismatch_count
        );
    }
}
