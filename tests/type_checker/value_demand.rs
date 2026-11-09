// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A call that returns nothing yields no value: every place that reads one
//! refuses it, and every place that does not read one still accepts it.

use crate::integration::utils::{
    assert_compiler_error, assert_runs_with_output, assert_runtime_error,
};

const NO_VALUE: &str = "produces no value";

/// The program whose `main` is `body`, beside a function returning nothing.
fn program(body: &str) -> String {
    let indented: String = body.lines().map(|line| format!("    {line}\n")).collect();
    format!(
        "use system.io\n\nfn nothing()\n    print(\"\")\n\n\
         struct Point\n    x int\n\n\
         class Box\n    public var x int\n    fn init(value int)\n        self.x = value\n\n\
         fn id<T>(value T) T\n    return value\n\n\
         fn main()\n{indented}"
    )
}

fn assert_refused(body: &str) {
    assert_compiler_error(&program(body), NO_VALUE);
}

#[test]
fn test_void_call_refused_as_let_initializer() {
    assert_refused("let y = nothing()");
}

#[test]
fn test_void_call_refused_as_var_initializer() {
    assert_refused("var w = nothing()");
}

#[test]
fn test_void_call_refused_as_assignment_source() {
    assert_compiler_error(&program("var a = 1\na = nothing()"), "cannot assign void");
}

#[test]
fn test_void_call_refused_as_list_element() {
    assert_refused("let z = [nothing(), nothing()]");
}

#[test]
fn test_void_call_refused_as_set_element() {
    assert_refused("let z = {nothing()}");
}

#[test]
fn test_void_call_refused_as_map_value() {
    assert_refused("let z = {\"a\": nothing()}");
}

#[test]
fn test_void_call_refused_as_map_key() {
    assert_refused("let z = {nothing(): 1}");
}

#[test]
fn test_void_call_refused_as_tuple_element() {
    assert_refused("let q = (nothing(), 1)");
}

#[test]
fn test_void_call_refused_as_equality_operand() {
    assert_refused("let k = nothing() == nothing()");
}

#[test]
fn test_void_call_refused_as_arithmetic_operand() {
    assert_refused("let k = 1 + nothing()");
}

#[test]
fn test_void_call_refused_as_logical_operand() {
    assert_refused("let k = true and nothing()");
}

#[test]
fn test_void_call_refused_as_unary_operand() {
    assert_compiler_error(&program("let k = not nothing()"), "got void");
}

#[test]
fn test_void_call_refused_as_range_bound() {
    assert_refused("let r = 0..nothing()");
}

#[test]
fn test_void_call_refused_as_generic_argument() {
    assert_refused("id(nothing())");
}

#[test]
fn test_void_call_refused_as_named_argument() {
    assert_compiler_error(&program("let b = Box(value: nothing())"), "got void");
}

#[test]
fn test_void_call_refused_as_struct_field_value() {
    assert_compiler_error(&program("let p = Point(x: nothing())"), "got void");
}

#[test]
fn test_void_call_refused_as_enum_payload() {
    assert_refused("let s = Some(nothing())");
}

#[test]
fn test_void_call_refused_in_format_string() {
    assert_compiler_error(
        &program("println(f\"{nothing()}\")"),
        "cannot be used in string interpolation",
    );
}

#[test]
fn test_void_call_refused_as_index() {
    assert_compiler_error(
        &program("let l = [1]\nlet v = l[nothing()]"),
        "index must be an integer",
    );
}

#[test]
fn test_void_call_refused_as_match_subject() {
    assert_refused("match nothing()\n    _: print(\"\")");
}

#[test]
fn test_void_call_refused_as_cast_operand() {
    assert_refused("let c = nothing() as int");
}

#[test]
fn test_void_condition_refused_when_every_branch_is_void() {
    assert_refused("let c = if true: nothing() else: nothing()");
}

#[test]
fn test_void_branch_refused_beside_a_value_branch() {
    assert_refused("let c = if true: nothing() else: 1");
}

#[test]
fn test_void_match_arm_refused_beside_a_value_arm() {
    assert_refused("let c = match 1\n    1: 2\n    _: nothing()");
}

#[test]
fn test_void_block_arm_refused_beside_a_value_arm() {
    assert_refused("let c = match 1\n    1: 2\n    _:\n        let a = 1\n        nothing()");
}

#[test]
fn test_void_argument_keeps_the_parameter_mismatch() {
    assert_compiler_error(
        &program("println(nothing())"),
        "Type mismatch for argument 'message': expected String, got void",
    );
}

#[test]
fn test_void_return_refused_from_function_returning_a_value() {
    assert_compiler_error(
        "fn nothing()\n    let a = 1\n\nfn f() int\n    return nothing()\n\nfn main()\n    f()\n",
        "void",
    );
}

#[test]
fn test_void_branch_refused_in_returned_conditional() {
    assert_compiler_error(
        "fn nothing()\n    let a = 1\n\nfn f() int\n    return nothing() if true else 1\n\nfn main()\n    f()\n",
        NO_VALUE,
    );
}

#[test]
fn test_void_arm_refused_in_tail_match() {
    assert_compiler_error(
        "fn nothing()\n    let a = 1\n\nfn f(n int) int\n    match n\n        1: 2\n        _: nothing()\n\nfn main()\n    f(1)\n",
        NO_VALUE,
    );
}

#[test]
fn test_void_branch_refused_in_expression_bodied_function() {
    assert_compiler_error(
        "fn nothing()\n    let a = 1\n\nfn f() int: if true: nothing() else: 1\n\nfn main()\n    f()\n",
        "got void",
    );
}

#[test]
fn test_void_branch_refused_in_lambda_tail() {
    assert_refused("let f = fn() int: if true: nothing() else: 1\nf()");
}

#[test]
fn test_void_call_runs_as_statement() {
    assert_runs_with_output(
        "use system.io\n\nfn nothing()\n    println(\"called\")\n\nfn main()\n    nothing()\n",
        "called",
    );
}

#[test]
fn test_void_call_returned_from_void_function() {
    assert_runs_with_output(
        "use system.io\n\nfn nothing()\n    println(\"called\")\n\nfn outer()\n    return nothing()\n\nfn main()\n    outer()\n",
        "called",
    );
}

#[test]
fn test_void_branches_run_as_statement() {
    assert_runs_with_output(
        "use system.io\n\nfn nothing()\n    println(\"called\")\n\nfn main()\n    if true: nothing() else: nothing()\n    match 1\n        1: nothing()\n        _: nothing()\n",
        "called\ncalled",
    );
}

#[test]
fn test_void_branch_runs_beside_value_branch_in_statement() {
    assert_runs_with_output(
        "use system.io\n\nfn nothing()\n    println(\"called\")\n\nfn main()\n    match 1\n        1: nothing()\n        _: 2\n",
        "called",
    );
}

#[test]
fn test_returning_arm_runs_beside_value_arm() {
    assert_runs_with_output(
        "use system.io\n\nfn f(n int) int\n    let c = match n\n        1: 10\n        _:\n            return 20\n    return c + 1\n\nfn main()\n    println(f\"{f(1)} {f(2)}\")\n",
        "11 20",
    );
}

#[test]
fn test_lambda_running_void_call_passed_to_generic() {
    assert_runs_with_output(
        "use system.io\n\nfn run<T>(f fn() T) T\n    return f()\n\nfn main()\n    run(fn(): println(\"called\"))\n",
        "called",
    );
}

#[test]
fn test_empty_map_literal_passed_where_typed_map_expected() {
    assert_runs_with_output(
        "use system.io\nuse system.collections.map\n\nfn size(m Map<String, int>) int\n    return m.length()\n\nfn main()\n    println(f\"{size({})}\")\n    let m Map<String, int> = {}\n    println(f\"{m.length()}\")\n",
        "0\n0",
    );
}

#[test]
fn test_empty_map_literal_compared_with_typed_map() {
    assert_runs_with_output(
        "use system.io\nuse system.collections.map\n\nfn main()\n    let m Map<String, int> = {}\n    println(f\"{m == {}}\")\n",
        "true",
    );
}

#[test]
fn test_empty_list_literal_is_a_value() {
    assert_runs_with_output(
        "use system.io\nuse system.collections.list\n\nfn main()\n    let l = List<int>([])\n    println(f\"{l.length()}\")\n",
        "0",
    );
}

/// `body` beside `system.io` and `system.process`, reading a `Result`.
fn diverging_program(body: &str) -> String {
    format!("use system.io\nuse system.process\n\n{body}\n")
}

const MATCH_WITH_PANIC_ARM: &str = "fn f(r Result<int, String>) int
    let x = match r
        Result.Ok(v): v
        Result.Err(e): panic(\"ba\" + \"d\")
    return x + 1
";

#[test]
fn test_panic_arm_yields_nothing_on_the_taken_path() {
    assert_runs_with_output(
        &diverging_program(&format!(
            "{MATCH_WITH_PANIC_ARM}\nfn main()\n    println(f\"{{f(Result.Ok(41))}}\")"
        )),
        "42",
    );
}

#[test]
fn test_panic_arm_fails_on_its_own_path() {
    assert_runtime_error(
        &diverging_program(&format!(
            "{MATCH_WITH_PANIC_ARM}\nfn main()\n    println(f\"{{f(Result.Err('no'))}}\")"
        )),
        "bad",
    );
}

#[test]
fn test_exit_arm_yields_nothing_on_the_taken_path() {
    assert_runs_with_output(
        &diverging_program(
            "fn main()\n    let x = match 1\n        1: 7\n        _: exit(3)\n    println(f\"{x}\")",
        ),
        "7",
    );
}

#[test]
fn test_panic_branch_of_if_expression() {
    let program = |condition: &str| {
        diverging_program(&format!(
            "fn main()\n    let x = if {condition}: 5 else: panic(\"ba\" + \"d\")\n    println(f\"{{x}}\")"
        ))
    };
    assert_runs_with_output(&program("true"), "5");
    assert_runtime_error(&program("false"), "bad");
}

#[test]
fn test_panic_ending_a_block_arm() {
    let program = |n: i32| {
        diverging_program(&format!(
            "fn main()\n    let x = match {n}\n        1: 5\n        _:\n            println(\"failing\")\n            panic(\"ba\" + \"d\")\n    println(f\"{{x}}\")"
        ))
    };
    assert_runs_with_output(&program(1), "5");
    assert_runtime_error(&program(2), "bad");
}

const MARKED_FUNCTION: &str = "@diverges
fn fatal(message String)
    panic(message)
";

#[test]
fn test_user_function_marked_diverges_serves_as_an_arm() {
    let program = |n: i32| {
        diverging_program(&format!(
            "{MARKED_FUNCTION}\nfn main()\n    let x = match {n}\n        1: 5\n        _: fatal(\"ba\" + \"d\")\n    println(f\"{{x}}\")"
        ))
    };
    assert_runs_with_output(&program(1), "5");
    assert_runtime_error(&program(2), "bad");
}

#[test]
fn test_function_ending_in_panic_needs_no_return() {
    let program = |n: i32| {
        diverging_program(&format!(
            "fn f(n int) int\n    if n == 1\n        return 5\n    panic(\"ba\" + \"d\")\n\nfn main()\n    println(f\"{{f({n})}}\")"
        ))
    };
    assert_runs_with_output(&program(1), "5");
    assert_runtime_error(&program(2), "bad");
}

#[test]
fn test_expression_bodied_function_of_panic_needs_no_return() {
    assert_runtime_error(
        &diverging_program(
            "fn f() int: panic(\"ba\" + \"d\")\n\nfn main()\n    println(f\"{f()}\")",
        ),
        "bad",
    );
}

#[test]
fn test_shadowing_a_marked_name_does_not_inherit_the_mark() {
    assert_compiler_error(
        "fn panic(message String)\n    let a = message\n\nfn main()\n    let x = match 1\n        1: 5\n        _: panic(\"ba\" + \"d\")\n",
        NO_VALUE,
    );
}

#[test]
fn test_panic_refused_as_initializer() {
    assert_compiler_error(
        &diverging_program("fn main()\n    let x = panic(\"a\")"),
        NO_VALUE,
    );
}

#[test]
fn test_every_branch_diverging_supplies_no_value() {
    assert_compiler_error(
        &diverging_program("fn main()\n    let x = if true: panic(\"a\") else: panic(\"b\")"),
        NO_VALUE,
    );
}

#[test]
fn test_arm_ending_in_if_statement_refused_as_value() {
    assert_compiler_error(
        &diverging_program(
            "fn main()\n    let x = match 1\n        1: 2\n        _:\n            if true\n                print(\"\")\n    println(f\"{x}\")",
        ),
        NO_VALUE,
    );
}

#[test]
fn test_arm_ending_in_for_statement_refused_as_value() {
    assert_compiler_error(
        &diverging_program(
            "fn main()\n    let x = match 1\n        1: 2\n        _:\n            for i in 0..2\n                print(\"\")\n    println(f\"{x}\")",
        ),
        NO_VALUE,
    );
}

#[test]
fn test_arm_ending_in_if_whose_branches_all_return_runs() {
    assert_runs_with_output(
        &diverging_program(
            "fn f(n int) int\n    let x = match n\n        1: 2\n        _:\n            if n > 5\n                return 10\n            else\n                return 20\n    return x\n\nfn main()\n    println(f\"{f(1)} {f(9)}\")",
        ),
        "2 10",
    );
}

#[test]
fn test_diverges_refused_on_a_body_that_can_finish() {
    assert_compiler_error(
        &diverging_program("@diverges\nfn fatal()\n    print(\"\")\n\nfn main()\n    fatal()"),
        "must not finish",
    );
}

#[test]
fn test_diverges_refused_on_a_body_that_returns() {
    assert_compiler_error(
        &diverging_program(
            "@diverges\nfn fatal(n int)\n    if n == 1\n        return\n    panic(\"ba\" + \"d\")\n\nfn main()\n    fatal(1)",
        ),
        "must not finish",
    );
}
