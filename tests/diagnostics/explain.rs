// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::diagnostics::DiagnosticCode;

#[test]
fn test_every_code_has_nonzero_rule_and_reference() {
    for code in DiagnosticCode::all() {
        let explanation = code.explanation();
        let code_str = code.as_str();

        assert!(
            !explanation.rule.is_empty(),
            "code {} has an empty rule section",
            code_str
        );
        assert!(
            !explanation.rule.trim().is_empty(),
            "code {} has a whitespace-only rule section",
            code_str
        );

        assert!(
            explanation.reference.is_some(),
            "code {} has no reference section",
            code_str
        );
        let reference = explanation.reference.as_ref().unwrap();
        assert!(
            !reference.is_empty(),
            "code {} has an empty reference",
            code_str
        );
    }
}

#[test]
fn test_live_codes_have_example_before_and_after() {
    for code in DiagnosticCode::all() {
        if code.is_reserved() {
            continue;
        }

        let explanation = code.explanation();
        let code_str = code.as_str();

        assert!(
            explanation.example_before.is_some(),
            "live code {} has no example_before",
            code_str
        );
        let before = explanation.example_before.as_ref().unwrap();
        assert!(
            !before.is_empty(),
            "live code {} has an empty example_before",
            code_str
        );

        assert!(
            explanation.example_after.is_some(),
            "live code {} has no example_after",
            code_str
        );
        let after = explanation.example_after.as_ref().unwrap();
        assert!(
            !after.is_empty(),
            "live code {} has an empty example_after",
            code_str
        );
    }
}

#[test]
fn test_reserved_codes_have_no_examples() {
    for code in DiagnosticCode::all() {
        if !code.is_reserved() {
            continue;
        }

        let explanation = code.explanation();
        let code_str = code.as_str();

        assert!(
            explanation.example_before.is_none(),
            "reserved code {} should have no example_before",
            code_str
        );
        assert!(
            explanation.example_after.is_none(),
            "reserved code {} should have no example_after",
            code_str
        );
    }
}

#[test]
fn test_reference_paths_resolve_to_files() {
    use std::path::{Component, PathBuf};

    fn normalize_path(path: &PathBuf) -> PathBuf {
        let mut components = path.components().peekable();
        let mut ret = if let Some(c @ Component::Prefix(..)) = components.peek().copied() {
            components.next();
            PathBuf::from(c.as_os_str())
        } else {
            PathBuf::new()
        };

        for component in components {
            match component {
                Component::Prefix(..) => unreachable!(),
                Component::RootDir => {
                    ret.push(component.as_os_str());
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    ret.pop();
                }
                Component::Normal(c) => {
                    ret.push(c);
                }
            }
        }
        ret
    }

    for code in DiagnosticCode::all() {
        let explanation = code.explanation();
        if let Some(reference_path) = &explanation.reference {
            let code_str = code.as_str();

            let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
            let docs_diagnostics = manifest_dir.join("docs").join("diagnostics");
            let full_path = docs_diagnostics.join(reference_path);
            let normalized = normalize_path(&full_path);

            assert!(
                normalized.exists(),
                "code {} references {} (resolved to {}) which does not exist",
                code_str,
                reference_path,
                normalized.display()
            );
        }
    }
}

/// The documentation format is a contract, not a convention: the explain
/// command splits on these headings, so a file that drifts from them silently
/// loses a section rather than failing loudly.
#[test]
fn test_section_headings_are_exactly_as_contracted() {
    for code in DiagnosticCode::all() {
        let headings: Vec<&str> = code
            .doc()
            .lines()
            .filter_map(|line| line.strip_prefix("## "))
            .map(str::trim)
            .collect();

        // A retired code documents the rule and where to read more. It carries
        // no example pair, because the check it named no longer runs and there
        // is nothing to reproduce.
        let expected: &[&str] = if code.is_reserved() {
            &["Rule", "Reference"]
        } else {
            &["Rule", "Before", "After", "Reference"]
        };

        assert_eq!(
            headings,
            expected,
            "{} has the wrong sections; the explain command parses these by name and order",
            code.as_str()
        );
    }
}

/// The repair identifiers `MER_PAR_001`'s page names must all be real.
///
/// That page lists the constructs from other languages the parser recognises,
/// and names the repair each one offers. The names are the wire strings tooling
/// matches on, so a page naming a repair the binary does not ship would send a
/// reader looking for something that cannot arrive.
#[test]
fn test_the_repairs_the_unexpected_token_page_names_are_all_shipped() {
    use miri::diagnostics::repair::RepairId;

    let shipped: Vec<&str> = RepairId::all().iter().map(|id| id.as_str()).collect();
    let page = DiagnosticCode::ParUnexpectedToken.doc();

    let named: Vec<&str> = page
        .lines()
        .filter(|line| line.starts_with('|'))
        .filter_map(|line| line.split('|').nth(3))
        .map(str::trim)
        .filter_map(|cell| cell.strip_prefix('`')?.strip_suffix('`'))
        .collect();

    assert!(
        !named.is_empty(),
        "the page should name the repairs its recognised constructs offer"
    );

    for repair in named {
        assert!(
            shipped.contains(&repair),
            "the page names `{}`, which is not a repair this binary ships: {:?}",
            repair,
            shipped
        );
    }
}
