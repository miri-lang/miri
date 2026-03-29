// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::{
    assert_function_parameter_count, assert_nested_for_structure, assert_nested_if_structure,
    assert_nested_while_structure, assert_statement_count, parser_test,
};
use miri::ast::factory::{binary, expression_statement, int_literal_expression};
use miri::ast::BinaryOp;

#[test]
fn test_deeply_nested_binary_expression() {
    // Tests parser's ability to handle deeply nested binary expressions
    // without stack overflow or performance degradation
    let depth = 20;
    let mut input = "1".to_string();
    for _ in 0..depth {
        input.push_str(" + 1");
    }

    let mut expected = int_literal_expression(1);
    for _ in 0..depth {
        expected = binary(expected, BinaryOp::Add, int_literal_expression(1));
    }

    parser_test(&input, vec![expression_statement(expected)]);
}

#[test]
fn test_deeply_nested_parentheses() {
    // Tests parser's handling of deeply nested parenthesized expressions
    let depth = 20;
    let mut input = "1".to_string();
    for _ in 0..depth {
        input = format!("({})", input);
    }

    // Parser automatically unwraps parentheses in AST, so expected result is just the literal
    let expected = int_literal_expression(1);

    parser_test(&input, vec![expression_statement(expected)]);
}

#[test]
fn test_deeply_nested_while_loops() {
    // Tests parser's ability to handle deeply nested while loops using inline syntax
    let depth = 20;

    // Build nested while loops using inline colon syntax: while true: while true: ... break
    let mut input = "break".to_string();
    for _ in 0..depth {
        input = format!("while true: {}", input);
    }
    input = format!("\n{}\n", input);

    assert_nested_while_structure(&input, depth);
}

#[test]
fn test_deeply_nested_if_statements() {
    // Tests parser's ability to handle deeply nested if statements using inline syntax
    let depth = 20;

    // Build nested if statements: if true: if true: ... 1
    let mut input = "1".to_string();
    for _ in 0..depth {
        input = format!("if true: {}", input);
    }
    input = format!("\n{}\n", input);

    assert_nested_if_structure(&input, depth);
}

#[test]
fn test_deeply_nested_for_loops() {
    // Tests parser's ability to handle deeply nested for loops using inline syntax
    let depth = 15;

    // Build nested for loops: for x0 in items: for x1 in items: ... // end
    let mut input = "// end".to_string();
    for i in (0..depth).rev() {
        input = format!("for x{} in items: {}", i, input);
    }
    input = format!("\n{}\n", input);

    assert_nested_for_structure(&input, depth);
}

#[test]
fn test_large_file_many_statements() {
    // Tests parser's ability to handle files with many statements
    let count = 100;
    let mut input = String::new();
    for i in 0..count {
        input.push_str(&format!("let x{} = 1\n", i));
    }

    assert_statement_count(&input, count);
}

#[test]
fn test_many_parameters_function() {
    // Tests function declarations with many parameters
    let param_count = 50;
    let params: Vec<String> = (0..param_count).map(|i| format!("p{} int", i)).collect();
    let input = format!("\nfn many_params({})\n    // body\n", params.join(", "));

    assert_function_parameter_count(&input, param_count);
}

#[test]
fn test_many_function_declarations() {
    // Tests files with many function declarations
    let count = 50;
    let mut input = String::new();
    for i in 0..count {
        input.push_str(&format!("\nfn func{}()\n    // body\n", i));
    }

    assert_statement_count(&input, count);
}

#[test]
fn test_very_long_identifier() {
    // Tests that the parser can handle very long variable names
    let long_name: String = "x".repeat(1000);
    let input = format!("let {} = 42", long_name);

    assert_statement_count(&input, 1);
}

#[test]
fn test_very_long_string_literal() {
    // Tests parsing of very long string literals
    let long_content: String = "a".repeat(10000);
    let input = format!("let s = \"{}\"", long_content);

    assert_statement_count(&input, 1);
}

#[test]
fn test_many_list_elements() {
    // Tests list literals with many elements
    let count = 100;
    let elements: Vec<String> = (0..count).map(|i| i.to_string()).collect();
    let input = format!("let list = [{}]", elements.join(", "));

    assert_statement_count(&input, 1);
}

#[test]
fn test_many_map_entries() {
    // Tests map literals with many entries
    let count = 100;
    let entries: Vec<String> = (0..count).map(|i| format!("\"k{}\": {}", i, i)).collect();
    let input = format!("let map = {{{}}}", entries.join(", "));

    assert_statement_count(&input, 1);
}

#[test]
fn test_mixed_nested_control_flow() {
    // Tests combination of different control structures nested together
    let input = r#"
if condition1
    while running
        for i in items
            if condition2
                break
"#;

    assert_statement_count(input, 1);
}

#[test]
fn test_deeply_nested_match_in_loops() {
    // Tests match expressions nested within loops
    let input = r#"
while true
    for item in list
        match item
            1: 'one'
            2: 'two'
            default: 'other'
"#;

    assert_statement_count(input, 1);
}

#[test]
fn test_complex_chained_method_calls() {
    // Tests long chains of method/property access
    let input = "result = obj.method1().field.method2().method3().final_field";

    assert_statement_count(input, 1);
}

#[test]
fn test_complex_arithmetic_precedence() {
    // Tests complex expression with various operators and precedence
    // Note: This language doesn't have ** exponentiation operator
    let input = "result = 1 + 2 * 3 - 4 / 5 % 6 + (8 - 9) * 10";

    assert_statement_count(input, 1);
}

#[test]
fn test_deeply_nested_ternary_expressions() {
    // Tests nested conditional/ternary expressions
    let depth = 10;

    // Build: 1 if c else (2 if c else (3 if c else ... n))
    let mut input = format!("{}", depth + 1);
    for i in (1..=depth).rev() {
        input = format!("{} if cond else ({})", i, input);
    }
    input = format!("result = {}", input);

    assert_statement_count(&input, 1);
}

#[test]
fn test_multiple_blank_lines() {
    // Tests that multiple blank lines between statements are handled
    let input = r#"
let a = 1



let b = 2




let c = 3
"#;

    assert_statement_count(input, 3);
}

#[test]
fn test_trailing_whitespace() {
    // Tests that trailing whitespace on lines is handled correctly
    let input = "let a = 1   \nlet b = 2\t\t\nlet c = 3   \n";

    assert_statement_count(input, 3);
}

#[test]
fn test_comments_between_statements() {
    // Tests that comments between statements don't break parsing
    let input = r#"
let a = 1
// This is a comment
let b = 2
// Another comment
// And another
let c = 3
"#;

    assert_statement_count(input, 3);
}

#[test]
fn test_empty_input() {
    // Tests parsing of empty input
    assert_statement_count("", 0);
}

#[test]
fn test_only_whitespace() {
    // Tests parsing of whitespace-only input
    assert_statement_count("   \n\n\t\t\n   ", 0);
}

// ---------------------------------------------------------------------------
// EOF without trailing newline
// ---------------------------------------------------------------------------

#[test]
fn test_no_trailing_newline_function_with_body() {
    // Function with a body statement, no trailing newline
    assert_statement_count("fn greet()\n    let x = 1", 1);
}

#[test]
fn test_no_trailing_newline_trait_abstract_method() {
    // Trait with abstract method signature (no body), no trailing newline.
    // This was the exact trigger for the original parser bug.
    assert_statement_count("trait Printable\n    fn print()", 1);
}

#[test]
fn test_no_trailing_newline_trait_two_methods() {
    // Multiple abstract methods in a trait, no trailing newline
    assert_statement_count(
        "trait Serializable\n    fn serialize() String\n    fn size() int",
        1,
    );
}

#[test]
fn test_no_trailing_newline_class_with_field_and_method() {
    // Class with a field and a method, no trailing newline
    assert_statement_count(
        "class Dog\n    let name String\n    fn bark()\n        let x = 1",
        1,
    );
}

#[test]
fn test_no_trailing_newline_nested_if() {
    // Nested control flow, file ends at the innermost level
    assert_statement_count("if true\n    if false\n        let x = 1", 1);
}

#[test]
fn test_no_trailing_newline_multiple_top_level() {
    // Multiple top-level statements, last one has an indented body with no newline
    assert_statement_count("let a = 1\nfn f()\n    let b = 2", 2);
}

#[test]
fn test_only_comments() {
    // Tests parsing of comment-only input
    let input = r#"
// Just a comment
// And another
// More comments
"#;

    assert_statement_count(input, 0);
}

#[test]
fn test_single_expression() {
    // Tests minimal valid program - just an expression
    let input = "42";

    assert_statement_count(input, 1);
}

#[test]
fn test_deeply_nested_lambda() {
    // Tests nested lambda expressions (curried functions)
    // Using the correct lambda syntax: fn(): fn(): ...
    let input = "let f = fn(): fn(): fn(): fn(): fn(): 42";

    assert_statement_count(input, 1);
}

#[test]
fn test_lambda_with_many_parameters() {
    // Tests lambda with many parameters
    // Lambda parameters need types in this language
    let param_count = 20;
    let params: Vec<String> = (0..param_count).map(|i| format!("p{} int", i)).collect();
    let input = format!("let f = fn({}): p0 + p1", params.join(", "));

    assert_statement_count(&input, 1);
}

#[test]
fn test_class_with_many_members() {
    // Tests class with many method declarations
    let method_count = 30;
    let mut methods = String::new();
    for i in 0..method_count {
        methods.push_str(&format!("    fn method{}()\n        // body\n", i));
    }

    let input = format!("\nclass BigClass\n{}", methods);

    assert_statement_count(&input, 1);
}

#[test]
fn test_trait_with_many_abstract_methods() {
    // Tests trait with many abstract method declarations
    let method_count = 30;
    let mut methods = String::new();
    for i in 0..method_count {
        methods.push_str(&format!("    fn abstract_method{}()\n", i));
    }

    let input = format!("\ntrait BigTrait\n{}", methods);

    assert_statement_count(&input, 1);
}

#[test]
fn test_deep_inheritance_chain_declaration() {
    // Tests parsing of classes that extend other classes (deep inheritance chain definition)
    // Note: This tests parsing only, not type checking of the inheritance
    let input = r#"
class Base
    fn method()
        // body

class Level1 extends Base
    fn method()
        // body

class Level2 extends Level1
    fn method()
        // body

class Level3 extends Level2
    fn method()
        // body
"#;

    assert_statement_count(input, 4);
}
