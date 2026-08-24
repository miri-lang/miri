// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::diagnostics::{DiagnosticCode, Severity};
use std::str::FromStr;

#[test]
fn test_all_codes_match_wire_format() {
    for code in DiagnosticCode::all() {
        let s = code.as_str();
        // Format: MER_<AREA>_<NUM> where AREA is 2-3 uppercase letters and NUM is 3 digits.
        // Reserved codes are marked with metadata, not with special naming.
        assert!(s.starts_with("MER_"), "code {} does not start with MER_", s);
        let parts: Vec<&str> = s.split('_').collect();

        // Should be exactly 3 parts (MER_AREA_NUM)
        assert_eq!(
            parts.len(),
            3,
            "code {} has {} parts, expected exactly 3",
            s,
            parts.len()
        );

        // Check format matches ^MER_[A-Z]{2,3}_[0-9]{3}$
        assert!(
            parts[0] == "MER",
            "code {} first part {} should be MER",
            s,
            parts[0]
        );

        // Check area part (parts[1])
        assert!(
            parts[1].len() >= 2 && parts[1].len() <= 3,
            "code {} area part {} has wrong length",
            s,
            parts[1]
        );
        assert!(
            parts[1].chars().all(|c| c.is_ascii_uppercase()),
            "code {} area part {} has non-uppercase",
            s,
            parts[1]
        );

        // Check number part (parts[2]) is exactly 3 digits
        assert_eq!(
            parts[2].len(),
            3,
            "code {} number part {} has wrong length",
            s,
            parts[2]
        );
        assert!(
            parts[2].chars().all(|c| c.is_ascii_digit()),
            "code {} number part {} has non-digit",
            s,
            parts[2]
        );
    }
}

#[test]
fn test_no_duplicate_wire_strings() {
    let mut seen = std::collections::HashSet::new();
    for code in DiagnosticCode::all() {
        let s = code.as_str();
        assert!(seen.insert(s), "duplicate wire string: {}", s);
    }
}

#[test]
fn test_round_trip_parse() {
    // Reserved codes round-trip too: `miri explain MER_LEX_013` must resolve to
    // the retired entry and report it as retired, not fail to parse.
    for code in DiagnosticCode::all() {
        let s = code.as_str();
        let parsed = DiagnosticCode::from_str(s).expect(&format!("failed to parse {}", s));
        assert_eq!(parsed, *code, "round-trip failed for {}", s);
    }
}

#[test]
fn test_parse_invalid_input() {
    let invalid_inputs: Vec<&str> = vec![
        "",
        "E0100",
        "MER_ZZZ_001",
        "MER_TYP_1",
        "MER_TYP_00100",
        "MERTYP001",
        "mer_typ_001",
    ];

    for input in invalid_inputs {
        assert!(
            DiagnosticCode::from_str(input).is_err(),
            "should reject input: {}",
            input
        );
    }

    // Also test a very long input
    let long_input = "x".repeat(5000);
    assert!(
        DiagnosticCode::from_str(&long_input).is_err(),
        "should reject long input"
    );
}

/// The exact set of retired codes. A reserved code names a check that no longer
/// exists: its number stays burned so it is never handed to a different
/// diagnosis. Pinning the whole set means flipping a flag on a live check — or
/// quietly retiring one — fails here instead of silently reshaping the registry.
const EXPECTED_RESERVED: &[&str] = &[
    // No lexer path produces these. An over-large integer literal is caught
    // later by the type checker and reported as `MER_TYP_068`; an unterminated
    // string is reported as `MER_LEX_001`.
    "MER_LEX_004",
    "MER_LEX_013",
    // The parser rejects a bad assignment target via `MER_PAR_004`, and never
    // constructs a standalone "unexpected operator".
    "MER_PAR_022",
    "MER_PAR_023",
    // The untyped escape hatches, retired once every call site was promoted to
    // a family code.
    "MER_TYP_028",
    "MER_MIR_015",
];

#[test]
fn test_reserved_set_is_exactly_as_pinned() {
    let mut actual: Vec<&str> = DiagnosticCode::all()
        .iter()
        .filter(|c| c.is_reserved())
        .map(|c| c.as_str())
        .collect();
    actual.sort_unstable();

    let mut expected: Vec<&str> = EXPECTED_RESERVED.to_vec();
    expected.sort_unstable();

    assert_eq!(
        actual, expected,
        "reserved set drifted; a live check must never be marked reserved, and a \
         retired one must never lose its burned number"
    );
}

#[test]
fn test_reserved_codes_still_carry_full_metadata() {
    // A retired code keeps its title and area so `miri explain` can say what it
    // used to mean rather than reporting an unknown code.
    for code in DiagnosticCode::all().iter().filter(|c| c.is_reserved()) {
        assert!(
            !code.title().is_empty(),
            "reserved code {} has no title",
            code.as_str()
        );
        assert!(
            !code.area().is_empty(),
            "reserved code {} has no area",
            code.as_str()
        );
    }
}

#[test]
fn test_per_area_numbering_is_gap_free() {
    use std::collections::HashMap;

    // Reserved codes are counted: a retired check keeps its number precisely so
    // the sequence stays dense and the number is never handed out again. A gap
    // means a number was skipped or silently dropped from the registry.
    let mut area_numbers: HashMap<&str, Vec<u32>> = HashMap::new();
    for code in DiagnosticCode::all() {
        let num: u32 = code
            .number()
            .parse()
            .unwrap_or_else(|_| panic!("code {} has a non-numeric number", code.as_str()));
        area_numbers.entry(code.area()).or_default().push(num);
    }

    for (area, mut numbers) in area_numbers {
        numbers.sort_unstable();
        assert_eq!(
            numbers[0], 1,
            "area {} starts at {} instead of 001",
            area, numbers[0]
        );
        for pair in numbers.windows(2) {
            assert_eq!(
                pair[1],
                pair[0] + 1,
                "area {} jumps from {:03} to {:03}; numbering must be dense",
                area,
                pair[0],
                pair[1]
            );
        }
    }
}

#[test]
fn test_display_impl() {
    for code in DiagnosticCode::all() {
        let displayed = format!("{}", code);
        assert_eq!(displayed, code.as_str());
    }
}

#[test]
fn test_copy_clone_debug_eq_hash() {
    let code = DiagnosticCode::LexInvalidToken;

    // Copy
    let _copied = code;
    let _copied2 = code;

    // Clone
    let _cloned = code.clone();

    // Debug
    let _ = format!("{:?}", code);

    // Eq/PartialEq
    assert_eq!(code, code);
    assert_eq!(code, DiagnosticCode::LexInvalidToken);

    // Hash
    let mut set = std::collections::HashSet::new();
    set.insert(code);
    assert!(set.contains(&code));
}

#[test]
fn test_all_codes_have_usable_metadata() {
    // Calling each accessor proves only that it does not panic. What matters is
    // that the values are usable: `miri explain` prints the title, and a code
    // whose title is blank or whose area disagrees with its own wire string
    // would render a diagnostic with no name on it.
    for code in DiagnosticCode::all() {
        let wire = code.as_str();

        assert!(!code.title().is_empty(), "code {wire} has an empty title");
        assert!(
            !code.title().trim().is_empty(),
            "code {wire} has a whitespace-only title"
        );

        let expected = format!("MER_{}_{}", code.area(), code.number());
        assert_eq!(
            wire, expected,
            "code {wire} disagrees with its own area/number metadata"
        );

        assert!(
            matches!(code.severity(), Severity::Error | Severity::Warning),
            "code {wire} has severity {:?}; a registry entry is never a bare note",
            code.severity()
        );
    }
}

#[test]
fn test_parse_rejects_well_formed_but_unregistered_codes() {
    // The dangerous input is not garbage — it is a string with exactly the right
    // shape that names no entry. `miri explain MER_LEX_099` must say "unknown",
    // never resolve to a neighbouring code.
    for input in [
        "MER_LEX_099",  // real area, number past the end of that area
        "MER_TYP_999",  // real area, number far past the end
        "MER_CG_001x",  // real prefix with trailing junk
        " MER_LEX_001", // leading space
        "MER_LEX_001 ", // trailing space
    ] {
        assert!(
            DiagnosticCode::from_str(input).is_err(),
            "well-formed but unregistered input {input:?} must not resolve"
        );
    }
}
