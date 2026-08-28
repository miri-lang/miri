// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Canonical formatter round-trip tests.
//!
//! The formatter's contract is that rendering is a fixed point: text rendered
//! from an AST parses back to a program that renders to the same text. That is
//! the property a tool relies on when it reads a declaration and later anchors
//! an edit against the bytes it read.

use miri::ast::formatter;
use miri::ast::Program;
use miri::lexer::Lexer;
use miri::parser::Parser;

/// Parse source the way `miri view` does: no normalization, no script-mode
/// wrapping, so the AST holds exactly what was written.
fn parse(source: &str) -> Result<Program, String> {
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    parser.parse().map_err(|error| format!("{:?}", error))
}

/// Render source and assert that re-parsing and re-rendering reproduces it.
fn assert_render_is_a_fixed_point(source: &str) -> String {
    let program = match parse(source) {
        Ok(program) => program,
        Err(error) => panic!("the fixture itself does not parse: {error}\n---\n{source}"),
    };
    let once = formatter::program(&program).text;

    let reparsed = match parse(&once) {
        Ok(program) => program,
        Err(error) => panic!("rendered text does not parse: {error}\n--- rendered ---\n{once}"),
    };
    let twice = formatter::program(&reparsed).text;

    assert_eq!(
        once, twice,
        "rendering is not a fixed point\n--- first ---\n{once}\n--- second ---\n{twice}"
    );
    once
}

/// Assert the AST survives a render/re-parse cycle unchanged.
fn assert_ast_survives(source: &str) {
    let before = parse(source).expect("fixture parses");
    let rendered = formatter::program(&before).text;
    let after = parse(&rendered).expect("rendered text parses");
    assert_eq!(
        before, after,
        "the AST changed across a render cycle\n--- rendered ---\n{rendered}"
    );
}

#[test]
fn test_function_with_a_body_round_trips() {
    let rendered = assert_render_is_a_fixed_point("fn main()\n    println(\"Hello, World!\")\n");
    assert!(rendered.contains("fn main()"), "got: {rendered}");
    assert_ast_survives("fn main()\n    println(\"Hello, World!\")\n");
}

#[test]
fn test_parameters_render_without_a_colon() {
    let rendered = assert_render_is_a_fixed_point("fn add(a int, b int) int\n    return a + b\n");
    assert!(
        rendered.contains("fn add(a int, b int) int"),
        "parameters must render as `name Type`, got: {rendered}"
    );
}

#[test]
fn test_bindings_render_without_a_colon() {
    let rendered =
        assert_render_is_a_fixed_point("fn main()\n    let total int = 1\n    var seen = 2\n");
    assert!(rendered.contains("let total int = 1"), "got: {rendered}");
    assert!(rendered.contains("var seen = 2"), "got: {rendered}");
}

#[test]
fn test_string_escapes_survive_a_round_trip() {
    assert_ast_survives("fn main()\n    let s = \"a\\nb\\tc\\\\d\\\"e\"\n    println(s)\n");
}

#[test]
fn test_float_literals_keep_their_decimal_point() {
    let rendered =
        assert_render_is_a_fixed_point("fn main()\n    let ratio = 1.0\n    println(ratio)\n");
    assert!(
        rendered.contains("1.0"),
        "a float must not render as an integer, got: {rendered}"
    );
    assert_ast_survives("fn main()\n    let ratio = 1.0\n    println(ratio)\n");
}

#[test]
fn test_nested_blocks_keep_their_indentation() {
    let source = "\
fn classify(value int) int
    if value > 0
        if value > 10
            return 2
        return 1
    else
        return 0
";
    let rendered = assert_render_is_a_fixed_point(source);
    assert!(
        rendered.contains("\n            return 2"),
        "got: {rendered}"
    );
    assert_ast_survives(source);
}

#[test]
fn test_precedence_is_preserved_without_redundant_parentheses() {
    let rendered =
        assert_render_is_a_fixed_point("fn main()\n    let x = 1 + 2 * 3\n    println(x)\n");
    assert!(
        rendered.contains("1 + 2 * 3"),
        "no parentheses are needed here, got: {rendered}"
    );
    assert_ast_survives("fn main()\n    let x = (1 + 2) * 3\n    println(x)\n");
}

#[test]
fn test_bitwise_operators_keep_their_additive_binding() {
    // `|`, `&` and `^` bind with `+`, not at the logical levels the `and` /
    // `or` keywords occupy, so parentheses around an additive operand must
    // survive: dropping them would re-associate the expression.
    assert_ast_survives("fn main()\n    let x = 1 & (2 + 3)\n    println(x)\n");
    assert_ast_survives("fn main()\n    let x = 1 | (2 - 3)\n    println(x)\n");
    assert_ast_survives("fn main()\n    let x = 1 ^ (2 + 3)\n    println(x)\n");
    assert_ast_survives("fn main()\n    let x = (1 & 2) + 3\n    println(x)\n");
}

#[test]
fn test_an_inline_body_stays_inline() {
    // `body <- COLON statement / block`: the parser records which form was
    // written, so rendering a colon body as a block would wrap it in a `Block`
    // it never had.
    let rendered = assert_render_is_a_fixed_point("fn one() int: 1\n");
    assert!(rendered.contains("fn one() int: 1"), "got: {rendered}");
    assert_ast_survives("fn one() int: 1\n");
}

#[test]
fn test_an_infinite_float_constant_round_trips() {
    // Infinity has no finite decimal spelling; it is reached by overflow.
    assert_ast_survives("const INF = 1e309\n");
}

#[test]
fn test_collection_types_render_in_source_syntax() {
    let rendered = assert_render_is_a_fixed_point(
        "fn totals(values [int], lookup {String: int}, tags {String}) [int]\n    return values\n",
    );
    assert!(rendered.contains("values [int]"), "got: {rendered}");
    assert!(rendered.contains("lookup {String: int}"), "got: {rendered}");
    assert!(rendered.contains("tags {String}"), "got: {rendered}");
}

#[test]
fn test_class_with_methods_round_trips() {
    let source = "\
class Point
    public x int
    public y int

    fn length() int
        return self.x + self.y
";
    assert_render_is_a_fixed_point(source);
}

#[test]
fn test_enum_round_trips() {
    let source = "\
enum Color
    Red
    Green
    Blue
";
    assert_render_is_a_fixed_point(source);
}

#[test]
fn test_match_round_trips() {
    let source = "\
fn name(value int) String
    match value
        1
            return \"one\"
        default
            return \"many\"
";
    assert_render_is_a_fixed_point(source);
}

#[test]
fn test_loops_round_trip() {
    let source = "\
fn main()
    for i in 0..10
        println(i)
    var n = 0
    while n < 3
        n = n + 1
";
    assert_render_is_a_fixed_point(source);
}

#[test]
fn test_spans_delimit_exactly_the_rendered_declaration() {
    let program = parse("fn first()\n    println(\"a\")\n\nfn second()\n    println(\"b\")\n")
        .expect("fixture parses");
    let rendered = formatter::program(&program);

    let names: Vec<_> = rendered
        .spans
        .iter()
        .filter_map(|span| span.name.clone())
        .collect();
    assert_eq!(names, vec!["first".to_string(), "second".to_string()]);

    for span in &rendered.spans {
        let slice = &rendered.text[span.start..span.end];
        let name = span.name.clone().unwrap_or_default();
        assert!(
            slice.starts_with(&format!("fn {name}")),
            "span for {name} does not delimit its declaration: {slice:?}"
        );
    }
}

#[test]
fn test_the_repository_corpus_renders_as_a_fixed_point() {
    let mut checked = 0;
    let mut failures = Vec::new();

    for path in miri_sources() {
        let source = std::fs::read_to_string(&path).expect("a listed source file is readable");
        // Only files the parser already accepts can say anything about the
        // formatter; a file that never parsed is not a formatter failure.
        let Ok(program) = parse(&source) else {
            continue;
        };
        checked += 1;

        let once = formatter::program(&program).text;
        match parse(&once) {
            Ok(reparsed) => {
                let twice = formatter::program(&reparsed).text;
                if once != twice {
                    failures.push(format!("{}: not a fixed point", path.display()));
                }
                // Idempotence alone would also hold for a formatter that
                // mangled a construct the same way every time, so the tree
                // itself has to come back equal.
                if program != reparsed {
                    failures.push(format!(
                        "{}: the AST changed across a render",
                        path.display()
                    ));
                }
            }
            Err(error) => failures.push(format!(
                "{}: rendered text rejected: {error}",
                path.display()
            )),
        }
    }

    assert!(checked > 0, "the corpus scan found no parseable sources");
    assert!(
        failures.is_empty(),
        "{} of {checked} corpus files failed:\n{}",
        failures.len(),
        failures.join("\n")
    );
}

/// Every `.mi` file in the repository's own source and example trees.
fn miri_sources() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for directory in ["src", "examples", "tests"] {
        collect(&root.join(directory), &mut found);
    }
    found.sort();
    found
}

/// Collect `.mi` files under `directory`, depth first.
fn collect(directory: &std::path::Path, found: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(&path, found);
        } else if path.extension().is_some_and(|extension| extension == "mi") {
            found.push(path);
        }
    }
}
