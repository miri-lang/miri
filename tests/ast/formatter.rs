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
    let once = formatter::program(&program, source).text;

    let reparsed = match parse(&once) {
        Ok(program) => program,
        Err(error) => panic!("rendered text does not parse: {error}\n--- rendered ---\n{once}"),
    };
    let twice = formatter::program(&reparsed, &once).text;

    assert_eq!(
        once, twice,
        "rendering is not a fixed point\n--- first ---\n{once}\n--- second ---\n{twice}"
    );
    once
}

/// Assert the AST survives a render/re-parse cycle unchanged.
fn assert_ast_survives(source: &str) {
    let before = parse(source).expect("fixture parses");
    let rendered = formatter::program(&before, source).text;
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
    let source = "fn first()\n    println(\"a\")\n\nfn second()\n    println(\"b\")\n";
    let program = parse(source).expect("fixture parses");
    let rendered = formatter::program(&program, source);

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
    on_a_deep_stack(|| {
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

            let once = formatter::program(&program, &source).text;
            match parse(&once) {
                Ok(reparsed) => {
                    let twice = formatter::program(&reparsed, &once).text;
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
    });
}

/// Every `.mi` file in the repository's own source and example trees.
fn miri_sources() -> Vec<std::path::PathBuf> {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut found = Vec::new();
    for directory in ["src", "examples", "tests", "conformance", "evals", "docs"] {
        collect(&root.join(directory), &mut found);
    }
    found.sort();
    found
}

/// Run `body` on a thread with room for the parser's recursion.
///
/// The corpus includes fixtures written to push the parser to its nesting
/// limit. The limit is reported as a diagnostic, but reaching it costs stack,
/// and a test thread's default is not enough to get there — the run aborts
/// before the parser can refuse.
fn on_a_deep_stack(body: impl FnOnce() + Send + 'static) {
    const STACK: usize = 16 * 1024 * 1024;
    std::thread::Builder::new()
        .stack_size(STACK)
        .spawn(body)
        .expect("the scan thread starts")
        .join()
        .expect("the scan thread finishes");
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

/// Every comment written in `source`, in the order the lexer meets them.
fn comments_in(source: &str) -> Vec<String> {
    let mut lexer = Lexer::new(source);
    for token in lexer.by_ref() {
        if token.is_err() {
            panic!("the fixture lexes: {source}");
        }
    }
    let mut found = lexer.take_leading_comments();
    found.extend(lexer.take_trailing_comments());
    // Deliberately unsorted: order is part of what must survive, so that a
    // render which keeps every comment but moves one is still a failure.
    found.into_iter().map(|comment| comment.text).collect()
}

#[test]
fn test_a_whole_program_renders_with_its_comments() {
    let source = "// what the helper is for\nfn helper() int\n    // the answer\n    return 1 // beside it\n";
    let program = parse(source).expect("the fixture parses");

    let rendered = formatter::program(&program, source).text;

    assert!(
        rendered.contains("// what the helper is for"),
        "a comment above a declaration survives: {rendered}"
    );
    assert!(
        rendered.contains("// the answer"),
        "a comment inside a body survives: {rendered}"
    );
    assert!(
        rendered.contains("// beside it"),
        "a comment after code survives: {rendered}"
    );
}

#[test]
fn test_a_single_declaration_renders_without_its_comments() {
    let source = "fn helper() int\n    // the answer\n    return 1\n";
    let program = parse(source).expect("the fixture parses");

    let rendered = formatter::declaration(&program.body[0]).text;

    assert!(
        !rendered.contains("// the answer"),
        "an anchor is matched against this text, so a comment must not appear \
         in it — otherwise `--old` could match inside a comment and edit it: {rendered}"
    );
}

#[test]
fn test_the_repository_corpus_keeps_every_comment_through_a_render() {
    on_a_deep_stack(|| {
        let mut checked = 0;
        let mut failures = Vec::new();

        for path in miri_sources() {
            let source = std::fs::read_to_string(&path).expect("a listed source file is readable");
            let Ok(program) = parse(&source) else {
                continue;
            };
            checked += 1;

            let rendered = formatter::program(&program, &source).text;
            let before = comments_in(&source);
            let after = comments_in(&rendered);
            if before != after {
                failures.push(format!(
                    "{}: {} comments in, {} out",
                    path.display(),
                    before.len(),
                    after.len()
                ));
            }
        }

        assert!(checked > 0, "the corpus scan found no parseable sources");
        assert!(
            failures.is_empty(),
            "{} of {checked} corpus files lost or gained a comment:\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}

#[test]
fn test_a_field_written_without_a_keyword_renders_without_one() {
    let source = "class Order\n    total int\n    var count int\n";
    let program = parse(source).expect("the fixture parses");

    let rendered = formatter::program(&program, source).text;

    assert!(
        rendered.contains("    total int"),
        "a field written with no mutability keyword keeps none: {rendered}"
    );
    assert!(
        !rendered.contains("var total"),
        "rendering must not invent a keyword the author did not write: {rendered}"
    );
    assert!(
        rendered.contains("    var count int"),
        "a field written `var` keeps it: {rendered}"
    );
}

#[test]
fn test_a_block_comment_survives_a_render() {
    // `fmt` rewrites a file from its tree. A comment form the tree does not
    // carry is a comment the rewrite deletes, so both forms are recorded.
    let source = "fn helper(a int) int\n    /* block note */\n    let t = a + 1\n    return t\n";
    let program = parse(source).expect("the fixture parses");

    let rendered = formatter::program(&program, source).text;

    assert!(
        rendered.contains("/* block note */"),
        "a block comment survives the render: {rendered}"
    );
}

#[test]
fn test_a_comment_below_the_last_statement_survives_a_render() {
    // Nothing follows these comments to claim them as leading, so without an
    // explicit home they would be dropped — and `fmt` rewrites from the tree,
    // so a dropped comment is a deleted one.
    let source = "fn add(a int, b int) int\n    return a + b\n    // tail of the body\n";
    let program = parse(source).expect("the fixture parses");

    let rendered = formatter::program(&program, source).text;

    assert!(
        rendered.contains("// tail of the body"),
        "the comment below the last statement survives: {rendered}"
    );
}

#[test]
fn test_alternative_patterns_render_with_the_bar_the_parser_reads() {
    // The parser separates a branch's alternative patterns with `|`. Rendering
    // them with a comma produced text the parser rejected, so `miri fmt`
    // refused every file holding such an arm.
    let rendered = assert_render_is_a_fixed_point(
        "
fn describe(n int) String
    match n
        1 | 2: \"small\"
        _: \"other\"
",
    );

    assert!(
        rendered.contains("1 | 2"),
        "alternatives belong to one branch and keep the bar, got:\n{rendered}"
    );
    assert!(
        !rendered.contains("1, 2"),
        "a comma would read as two branches of an inline match, got:\n{rendered}"
    );
}

#[test]
fn test_a_branch_with_alternatives_and_a_block_body_survives_a_render() {
    assert_ast_survives(
        "
fn describe(n int) String
    match n
        1 | 2 | 3:
            let label = \"small\"
            label
        _
            \"other\"
",
    );
}

#[test]
fn test_constructor_bindings_keep_their_commas() {
    // The comma inside a constructor pattern separates bindings rather than
    // alternatives, and must not follow the bar.
    let rendered = assert_render_is_a_fixed_point(
        "
fn describe(p Pair) int
    match p
        Pair.Both(a, b): a + b
        _: 0
",
    );

    assert!(
        rendered.contains("Both(a, b)"),
        "bindings stay comma-separated, got:\n{rendered}"
    );
}

#[test]
fn test_a_block_bodied_function_expression_keeps_its_block() {
    // A block holding one expression and a bare expression body are different
    // trees. Rendering the first as the second hands back a program that
    // parses to something else, which the fixed-point contract forbids.
    assert_ast_survives(
        "
fn main()
    let bare = fn(x int) int
        x + 100
    println(f\"{bare(2)}\")
",
    );
}

#[test]
fn test_an_inline_function_expression_body_stays_on_its_line() {
    let rendered = assert_render_is_a_fixed_point(
        "
fn main()
    let inline = fn(x int) int: x + 1
    println(f\"{inline(2)}\")
",
    );

    assert!(
        rendered.contains("fn(x int) int: x + 1"),
        "a body written on the header's line stays there, got:\n{rendered}"
    );
}

/// Every place the `public` keyword can be written, and the declaration it
/// precedes.
///
/// `public` is the default visibility, so nothing about the program changes
/// when it is dropped — which is exactly why nothing caught the formatter
/// dropping it. A file rewritten without it differs from the author's in every
/// declaration, and the difference is invisible to a gate that only asks
/// whether rendering reaches a fixed point.
const PUBLIC_DECLARATIONS: &[&str] = &[
    "public fn helper() int\n    1\n",
    "public const LIMIT = 4\n",
    "public let seed = 1\n",
    "public var count = 0\n",
    "public type Alias is int\n",
    "public enum Colour\n    Red\n    Green\n",
    "public struct Point\n    x int\n    y int\n",
    "public trait Named\n    fn name() String\n",
    "public class Holder\n    public var value int\n    public fn value_of() int: self.value\n",
];

#[test]
fn test_public_survives_a_round_trip() {
    for source in PUBLIC_DECLARATIONS {
        let rendered = assert_render_is_a_fixed_point(source);
        assert!(
            rendered.contains("public "),
            "the `public` the source wrote was dropped\n--- source ---\n{source}--- rendered ---\n{rendered}"
        );
    }
}

#[test]
fn test_a_declaration_written_without_public_does_not_gain_it() {
    let rendered = assert_render_is_a_fixed_point("fn helper() int\n    1\n");
    assert!(
        !rendered.contains("public"),
        "a keyword the source never wrote must not appear, got:\n{rendered}"
    );
}

#[test]
fn test_public_is_kept_on_every_member_of_a_class() {
    let rendered = assert_render_is_a_fixed_point(
        "public class Holder\n    private var hidden int\n    public var shown int\n    public fn shown_value() int: self.shown\n",
    );

    assert_eq!(
        rendered.matches("public ").count(),
        3,
        "each declaration keeps the keyword it was written with, got:\n{rendered}"
    );
    assert!(
        rendered.contains("private var hidden"),
        "private is unaffected, got:\n{rendered}"
    );
}

#[test]
fn test_abstract_survives_a_round_trip() {
    let rendered = assert_render_is_a_fixed_point(
        "public trait Sized\n    abstract fn length() int\n    fn is_empty() bool: self.length() == 0\n",
    );

    assert!(
        rendered.contains("abstract fn length()"),
        "the `abstract` the source wrote was dropped, got:\n{rendered}"
    );
    assert_eq!(
        rendered.matches("abstract").count(),
        1,
        "only the member written `abstract` gets the keyword, got:\n{rendered}"
    );
}

#[test]
fn test_an_abstract_class_keeps_the_keyword_on_class_and_method() {
    let rendered = assert_render_is_a_fixed_point(
        "abstract class Shape\n    abstract fn area() int\n    fn describe() String: \"shape\"\n",
    );

    assert!(
        rendered.contains("abstract class Shape"),
        "got:\n{rendered}"
    );
    assert!(rendered.contains("abstract fn area()"), "got:\n{rendered}");
}

#[test]
fn test_a_deprecated_attribute_keyword_is_written_back_not_deleted() {
    let source = "must_use enum Status\n    Ok\n    Failed\n";
    let rendered = assert_render_is_a_fixed_point(source);

    assert!(
        rendered.contains("must_use enum Status"),
        "the attribute must survive in the spelling it was written in, got:\n{rendered}"
    );
    assert_ast_survives(source);
}

#[test]
fn test_a_negation_of_a_negation_does_not_become_a_decrement() {
    let source = "fn main()\n    let x = 5\n    let y = - - x\n    println(f\"{y}\")\n";
    let rendered = assert_render_is_a_fixed_point(source);

    assert!(
        !rendered.contains("--"),
        "two negations written apart must not be joined into the decrement token, got:\n{rendered}"
    );
    assert_ast_survives(source);
}

#[test]
fn test_a_prefix_decrement_is_written_back_in_front_of_its_operand() {
    // The grammar has no postfix `--`, so rendering one produces text the
    // parser cannot read back. The operator is reported by the type checker,
    // not the parser, so the formatter still meets it.
    let source = "fn main()\n    var x = 10\n    let y = --x\n    println(f\"{y}\")\n";
    let rendered = assert_render_is_a_fixed_point(source);

    assert!(
        rendered.contains("--x"),
        "the operator stays in front of its operand, got:\n{rendered}"
    );
    assert_ast_survives(source);
}

#[test]
fn test_an_anonymous_parameter_is_not_preceded_by_a_space() {
    let rendered = assert_render_is_a_fixed_point(
        "fn apply(values [int], f fn(int, int) int) int\n    f(values[0], values[1])\n",
    );

    assert!(
        rendered.contains("fn(int, int) int"),
        "a parameter with no name has nothing to separate from its type, got:\n{rendered}"
    );
}

#[test]
fn test_a_bare_self_parameter_stays_bare() {
    let rendered = assert_render_is_a_fixed_point(
        "class Handle\n    var raw int\n    fn drop(self)\n        self.raw = 0\n",
    );

    assert!(
        rendered.contains("fn drop(self)"),
        "`self` carries its own type, so none is written, got:\n{rendered}"
    );
}

/// Numbers a source can spell in more than one way, each written the way an
/// author would write it.
///
/// The tree keeps a number's value, not its spelling, so rendering from the
/// tree alone replaces every one of these with the decimal it happens to
/// print. A file rewritten that way still runs and still means the same thing,
/// which is why nothing noticed: `0xFFFF` simply becomes `65535`, and the
/// reason the author wrote it in hex is gone.
const NUMERIC_SPELLINGS: &[&str] = &[
    "0b1010",
    "0o755",
    "0xFFFF",
    "1_000_000",
    "3.14159265358979323846",
    "0.00002",
    "1e300",
];

#[test]
fn test_a_number_keeps_the_spelling_it_was_written_with() {
    for spelling in NUMERIC_SPELLINGS {
        let source = format!("fn main()\n    let value = {}\n", spelling);
        let rendered = assert_render_is_a_fixed_point(&source);
        assert!(
            rendered.contains(spelling),
            "the spelling `{spelling}` was replaced, got:\n{rendered}"
        );
    }
}

#[test]
fn test_a_number_the_tree_alone_can_spell_is_unchanged() {
    let rendered = assert_render_is_a_fixed_point("fn main()\n    let value = 42\n");
    assert!(rendered.contains("let value = 42"), "got:\n{rendered}");
}

/// Whether the formatter is allowed to drop this word, because it writes the
/// type the word names in the sugar the language prefers: a built-in
/// collection renders `[T]`, `[T; N]`, `{K: V}` or `{T}` rather than by the
/// name of the class behind it.
///
/// That is the whole excuse. Every other word a source is written with has to
/// come back, which is what makes the gate below able to see a dropped
/// modifier: a `public` or an `abstract` the rendering does not write is a
/// word that went missing, and nothing excuses it.
fn rendered_as_sugar(word: &str) -> bool {
    miri::ast::types::BuiltinCollectionKind::from_name(word).is_some()
}

/// The words `text` is written with: every keyword, identifier and number in
/// it, in the order the lexer meets them.
///
/// Punctuation is left out because the formatter is allowed to move it — it
/// drops a grouping parenthesis that changes nothing and writes a body as an
/// indented block rather than after a colon. A word is different: the
/// formatter has no reason to write one the source did not, and no right to
/// drop one the source did.
fn words_in(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    for token in Lexer::new(text) {
        let Ok((_, span)) = token else {
            break;
        };
        let Some(lexeme) = text.get(span.start..span.end) else {
            continue;
        };
        let is_word = lexeme
            .chars()
            .next()
            .is_some_and(|first| first.is_alphanumeric() || first == '_')
            && lexeme
                .chars()
                .all(|c| c.is_alphanumeric() || c == '_' || c == '.');
        if is_word && !rendered_as_sugar(lexeme) {
            words.push(lexeme.to_string());
        }
    }
    words
}

#[test]
fn test_the_repository_corpus_keeps_every_word_it_was_written_with() {
    on_a_deep_stack(|| {
        let mut checked = 0;
        let mut failures = Vec::new();

        for path in miri_sources() {
            let source = std::fs::read_to_string(&path).expect("a listed source file is readable");
            let Ok(program) = parse(&source) else {
                continue;
            };
            checked += 1;

            let rendered = formatter::program(&program, &source).text;
            // Sorted, because a canonicalization may reorder words without
            // losing any: an `if x > a: x else: a` is written back in the
            // conditional-expression spelling, `x if x > a else a`. What the gate
            // is looking for is a word that went missing, not a word that moved.
            let mut before = words_in(&source);
            let mut after = words_in(&rendered);
            before.sort();
            after.sort();
            if before == after {
                continue;
            }
            let missing: Vec<&String> = before
                .iter()
                .filter(|word| !after.contains(word))
                .take(4)
                .collect();
            let added: Vec<&String> = after
                .iter()
                .filter(|word| !before.contains(word))
                .take(4)
                .collect();
            failures.push(format!(
                "{}: dropped {:?}, invented {:?}",
                path.display(),
                missing,
                added
            ));
        }

        assert!(checked > 0, "the corpus scan found no parseable sources");
        assert!(
            failures.is_empty(),
            "{} of {checked} corpus files came back missing a word they were written with:\n{}",
            failures.len(),
            failures.join("\n")
        );
    });
}
