// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Code generation reads MIR and the settled type facts the pipeline hands it,
//! never the type checker or MIR lowering directly. A backend that reached into
//! either would re-derive a rule those layers own and drift from it, so the
//! boundary is checked over the source rather than left to review.

use std::fs;
use std::path::{Path, PathBuf};

/// Module paths a codegen source file must not name outside a comment.
const FORBIDDEN: [&str; 2] = ["type_checker", "mir::lowering"];

/// Every `.rs` file under `dir`, recursively, in a stable order.
fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    while let Some(current) = pending.pop() {
        let entries = fs::read_dir(&current)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", current.display()));
        for entry in entries {
            let path = entry
                .unwrap_or_else(|e| panic!("cannot read an entry of {}: {e}", current.display()))
                .path();
            if path.is_dir() {
                pending.push(path);
            } else if path.extension().is_some_and(|ext| ext == "rs") {
                found.push(path);
            }
        }
    }
    found.sort();
    found
}

/// The code on `line`, without a comment it carries: nothing for a comment
/// line, and the text before `//` for a line ending in one.
fn code_part(line: &str) -> &str {
    line.split_once("//").map_or(line, |(code, _)| code)
}

#[test]
fn codegen_names_neither_the_type_checker_nor_mir_lowering() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let codegen = root.join("src").join("codegen");
    let mut violations = Vec::new();
    for path in rust_sources(&codegen) {
        let source = fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        for (index, line) in source.lines().enumerate() {
            let code = code_part(line);
            if FORBIDDEN.iter().any(|token| code.contains(token)) {
                let relative = path.strip_prefix(root).unwrap_or(&path);
                violations.push(format!(
                    "{}:{}: {}",
                    relative.display(),
                    index + 1,
                    line.trim()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "codegen must read MIR and `mir::type_facts`, not the type checker or MIR \
         lowering:\n{}",
        violations.join("\n")
    );
}

#[test]
fn a_trailing_comment_is_not_code() {
    assert_eq!(code_part("let x = 1; // mir::lowering"), "let x = 1; ");
    assert_eq!(code_part("/// type_checker"), "");
    assert_eq!(
        code_part("use crate::type_checker::X;"),
        "use crate::type_checker::X;"
    );
}
