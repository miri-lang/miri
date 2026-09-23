// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! How a source-derived name is spelled in the emitted WGSL.

use miri::codegen::wgsl::identifiers::source_identifier;

#[test]
fn ordinary_names_are_kept() {
    assert_eq!(source_identifier("state_a"), "state_a");
    assert_eq!(source_identifier("coef16"), "coef16");
}

#[test]
fn reserved_words_are_escaped() {
    assert_eq!(source_identifier("filter"), "_src_filter");
    assert_eq!(source_identifier("loop"), "_src_loop");
}

#[test]
fn predeclared_names_are_escaped() {
    assert_eq!(source_identifier("min"), "_src_min");
    assert_eq!(source_identifier("f16"), "_src_f16");
}

#[test]
fn names_in_the_synthesized_namespace_are_escaped() {
    assert_eq!(source_identifier("_inputs"), "_src__inputs");
    assert_eq!(source_identifier("_src_x"), "_src__src_x");
    assert_eq!(source_identifier("SUBGROUP_SIZE"), "_src_SUBGROUP_SIZE");
}

#[test]
fn escaping_keeps_distinct_names_distinct() {
    assert_ne!(
        source_identifier("filter"),
        source_identifier("_src_filter")
    );
    assert_ne!(source_identifier("_filter"), source_identifier("filter"));
}

/// Every word naga's WGSL front end reserves or predeclares is escaped, so the
/// hand-kept lists cannot drift behind the shader compiler that consumes them.
#[test]
fn every_word_naga_reserves_or_predeclares_is_escaped() {
    let words = naga::keywords::wgsl::RESERVED
        .iter()
        .chain(naga::keywords::wgsl::BUILTIN_IDENTIFIERS);
    for word in words {
        assert_ne!(
            source_identifier(word),
            *word,
            "`{}` reaches WGSL verbatim",
            word
        );
    }
}
