// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::statement::StatementKind;
use miri::error::diagnostic::Diagnostic;
use miri::error::{CompilerError, Span, TypeError};
use miri::pipeline::{program_uses_gpu, BuildOptions, Pipeline};

fn detects_gpu(source: &str) -> bool {
    let pipeline = Pipeline::new();
    let result = pipeline.frontend_script(source).expect("frontend");
    program_uses_gpu(result.ast.body.iter())
}

#[test]
fn program_uses_gpu_finds_gpu_for_at_top_level() {
    assert!(detects_gpu(
        "
use system.gpu
use system.collections.array

gpu var dst = [0, 0, 0, 0]
gpu forall i in 0..4
    dst[i] = i
"
    ));
}

#[test]
fn program_uses_gpu_walks_into_class_body() {
    assert!(detects_gpu(
        "
use system.gpu

class Worker
    gpu fn kernel()
        let x = 1
"
    ));
}

#[test]
fn program_uses_gpu_false_for_cpu_only_program() {
    assert!(!detects_gpu(
        "
use system.io
fn main()
    println(\"hello\")
"
    ));
}

/// Runs the script frontend on a program expected to fail type checking and
/// returns the errors and the warnings reported alongside them.
fn expect_type_errors(source: &str) -> (Vec<TypeError>, Vec<Diagnostic>) {
    match Pipeline::new().frontend_script(source) {
        Err(CompilerError::TypeErrors { errors, warnings }) => (errors, warnings),
        Err(other) => panic!("expected type errors, got {other:?}"),
        Ok(_) => panic!("expected the program to be rejected, but it type-checked"),
    }
}

/// The source text a span selects, as the diagnostic renderer would slice it.
fn spanned_text(source: &str, span: Span) -> &str {
    &source[span.start..span.end]
}

/// The rendered message of each type error, in report order.
fn error_messages(errors: &[TypeError]) -> Vec<String> {
    errors
        .iter()
        .map(|e| e.kind.properties().message.unwrap_or_default())
        .collect()
}

#[test]
fn frontend_reports_a_parser_error_for_an_unclosed_string() {
    let error = Pipeline::new()
        .frontend("fn main()\n    let s = \"unterminated\n")
        .expect_err("unterminated string literal must be rejected");

    match error {
        CompilerError::Parser(syntax) => {
            assert_eq!(syntax.count(), 1, "one declaration, one fault");
            // The span runs from the opening quote to the end of the line: an
            // unterminated literal is only recognised once the line ends, so
            // the newline that ended it is part of the reported text.
            assert_eq!(
                spanned_text(
                    "fn main()\n    let s = \"unterminated\n",
                    syntax.first().span
                ),
                "\"unterminated\n"
            );
        }
        other => panic!("expected a parser error, got {other:?}"),
    }
}

#[test]
fn frontend_reports_every_type_error_from_one_pass() {
    let source = "\
fn main()
    let a = 10
    let b = aa
    let c = bb
";
    let (errors, _) = expect_type_errors(source);

    assert_eq!(
        error_messages(&errors),
        vec!["Undefined variable: aa", "Undefined variable: bb"],
        "both undefined variables must be reported, in source order"
    );
    let spans: Vec<&str> = errors
        .iter()
        .map(|e| spanned_text(source, e.span))
        .collect();
    assert_eq!(spans, vec!["aa", "bb"]);
}

#[test]
fn type_errors_carry_the_warnings_reported_before_them() {
    let source = "\
fn main()
    var x = 5
    let y = --x
    let z = missing
";
    let (errors, warnings) = expect_type_errors(source);

    assert_eq!(errors.len(), 1, "one undefined variable");
    let codes: Vec<&str> = warnings.iter().filter_map(|w| w.code).collect();
    assert_eq!(
        codes,
        vec!["MER_TYP_024"],
        "the decrement warning must survive the failing check, not be dropped with it"
    );
    assert_eq!(
        spanned_text(source, warnings[0].span.expect("warning span")),
        "--x"
    );
}

#[test]
fn a_successful_check_exposes_its_warnings_on_the_type_checker() {
    let source = "\
fn main()
    var x = 5
    let y = --x
    return y
";
    let result = Pipeline::new()
        .frontend_script(source)
        .expect("program with only warnings must type-check");

    let codes: Vec<&str> = result
        .type_checker
        .warnings()
        .iter()
        .filter_map(|w| w.code)
        .collect();
    assert_eq!(codes, vec!["MER_TYP_024"]);
}

#[test]
fn a_type_error_span_selects_exactly_the_offending_text() {
    let source = "\
fn main()
    let total = 1
    let n = totl
";
    let (errors, _) = expect_type_errors(source);

    assert_eq!(spanned_text(source, errors[0].span), "totl");
}

#[test]
fn a_type_error_span_is_exact_on_a_final_line_without_a_newline() {
    let source = "fn main()\n    let n = missing";
    let (errors, _) = expect_type_errors(source);

    let span = errors[0].span;
    assert_eq!(spanned_text(source, span), "missing");
    assert_eq!(
        span.end,
        source.len(),
        "a span ending at EOF must stay inside the source"
    );
}

#[test]
fn a_type_error_span_counts_bytes_past_multi_byte_text() {
    let source = "\
fn main()
    let greeting = \"héllo wörld\"
    let n = missing
";
    let (errors, _) = expect_type_errors(source);

    assert_eq!(
        spanned_text(source, errors[0].span),
        "missing",
        "spans are byte offsets: a char-offset span would slice into the wrong text"
    );
}

#[test]
fn a_type_error_span_counts_bytes_past_a_multi_byte_comment() {
    let source = "\
// naïve — résumé
fn main()
    let n = missing
";
    let (errors, _) = expect_type_errors(source);

    assert_eq!(spanned_text(source, errors[0].span), "missing");
}

/// Names of the top-level function declarations in a frontend result.
fn top_level_functions(program: &miri::ast::Program) -> Vec<&str> {
    program
        .body
        .iter()
        .filter_map(|stmt| {
            let StatementKind::FunctionDeclaration(decl) = &stmt.node else {
                return None;
            };
            Some(decl.name.as_str())
        })
        .collect()
}

#[test]
fn the_plain_frontend_leaves_a_bare_script_unwrapped() {
    let result = Pipeline::new()
        .frontend("let x = 1\n")
        .expect("a bare script type-checks");

    assert!(
        top_level_functions(&result.ast).is_empty(),
        "frontend must not synthesize a main; only frontend_script wraps"
    );
}

#[test]
fn the_script_frontend_wraps_bare_statements_in_main() {
    let result = Pipeline::new()
        .frontend_script("let x = 1\n")
        .expect("a bare script type-checks");

    assert_eq!(top_level_functions(&result.ast), vec!["main"]);
}

#[test]
fn the_script_frontend_wraps_empty_source_in_main() {
    let result = Pipeline::new()
        .frontend_script("")
        .expect("empty source type-checks");

    assert_eq!(top_level_functions(&result.ast), vec!["main"]);
}

#[test]
fn the_script_frontend_does_not_wrap_a_program_that_has_main() {
    let result = Pipeline::new()
        .frontend_script("fn helper() int\n    return 1\n\nfn main()\n    let x = helper()\n")
        .expect("program with main type-checks");

    assert_eq!(top_level_functions(&result.ast), vec!["helper", "main"]);
}

#[test]
fn source_path_is_none_until_it_is_configured() {
    assert_eq!(Pipeline::new().source_path(), None);
    assert_eq!(
        Pipeline::new()
            .with_source_path("/tmp/program.mi".to_string())
            .source_path(),
        Some("/tmp/program.mi")
    );
}

#[test]
fn build_propagates_the_frontend_error_without_writing_an_artifact() {
    let out_dir = tempfile::tempdir().expect("temp dir");
    let out_path = out_dir.path().join("program");
    let opts = BuildOptions {
        out_path: Some(out_path.clone()),
        ..BuildOptions::default()
    };

    let error = Pipeline::new()
        .build("fn main()\n    let x = missing\n", &opts)
        .expect_err("a rejected program must not build");

    assert!(
        matches!(error, CompilerError::TypeErrors { .. }),
        "expected the frontend error to reach the caller, got {error:?}"
    );
    assert!(
        !out_path.exists(),
        "no artifact may be written for a rejected program"
    );
}

#[test]
fn get_mir_reports_the_type_error_instead_of_emitting_mir() {
    let error = Pipeline::new()
        .get_mir("fn main()\n    let x = missing\n")
        .expect_err("a rejected program has no MIR");

    assert!(
        matches!(error, CompilerError::TypeErrors { .. }),
        "expected a type error, got {error:?}"
    );
}

fn lowered_body_names(source: &str) -> Vec<String> {
    Pipeline::new()
        .get_gpu_mir_bodies(source)
        .expect("lowering")
        .into_iter()
        .map(|(name, _)| name)
        .collect()
}

/// A method of a generic class compiled at an instance was checked there, and
/// a method is checked at an instance only where the program uses it. So a
/// method nothing calls is not compiled for a scalar instance either: its body
/// may apply an operator the argument does not support.
#[test]
fn a_method_nothing_calls_is_not_compiled_for_a_scalar_instance() {
    let names = lowered_body_names(
        r#"
use system.io

class Box<T>
    v T
    fn lt() bool
        return self.v < "a"
    fn get() T
        return self.v

fn main()
    let b = Box<int>(v: 3)
    println(f"{b.get()}")
"#,
    );
    assert!(
        names.iter().any(|name| name.ends_with("Box$int.get")),
        "the called method is compiled at the instance: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.ends_with("Box$int.lt")),
        "an uncalled method must not be compiled at the instance: {names:?}"
    );
}

/// A class's copy of a trait default reads the trait's parameters at what the
/// class's clauses pin. Nothing calls this one, so its obligation on `int` is
/// never checked, and it is not compiled: `int` has no `<` against a string.
#[test]
fn a_trait_default_nothing_calls_is_not_compiled_for_a_class_pinning_it() {
    let names = lowered_body_names(
        r#"
use system.io

trait Op<T>
    fn keep(a T) T
    fn bad(a T) bool
        return a < "a"

class A implements Op<int>
    fn keep(a int) int
        return a

fn main()
    let x Op<int> = A()
    println(f"{x.keep(3)}")
"#,
    );
    assert!(
        names.iter().any(|name| name.ends_with("A.keep")),
        "the called method is compiled: {names:?}"
    );
    assert!(
        !names.iter().any(|name| name.ends_with("A.bad")),
        "an uncalled default failing at the class's pins must not be compiled: {names:?}"
    );
}

/// A body the type checker never judged at the types its operands have — a
/// generic method lowered where nothing checked it — applies an operator to a
/// string. Lowering it as a machine instruction would compare the string's
/// address as a number; it is lowered as the trap an unchecked method
/// reports instead, and no body keeps an arithmetic or ordering instruction
/// on a string.
#[test]
fn an_operator_nothing_checked_on_a_string_is_lowered_as_a_trap() {
    let bodies = Pipeline::new()
        .get_gpu_mir_bodies(
            r#"
use system.io

class Box<T>
    v T
    fn lt() bool
        return self.v < "a"
    fn get() T
        return self.v

fn main()
    let b = Box<int>(v: 3)
    println(f"{b.get()}")
"#,
        )
        .expect("lowering");
    let text: Vec<(String, String)> = bodies
        .iter()
        .map(|(name, body)| (name.clone(), body.to_string()))
        .collect();
    let lt: Vec<&(String, String)> = text
        .iter()
        .filter(|(name, _)| name.ends_with(".lt"))
        .collect();
    assert!(
        !lt.is_empty(),
        "a body of `lt` is lowered: {:?}",
        text.iter().map(|(n, _)| n).collect::<Vec<_>>()
    );
    for (name, body) in lt {
        assert!(
            body.contains("miri_rt_method_not_checked_panic"),
            "{name} reports the unchecked operator:\n{body}"
        );
    }
}
