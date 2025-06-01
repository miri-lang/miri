use miri::lexer::{Lexer, Token};
use miri::parser::*;

#[test]
fn test_parse_literal() {
    // A simple integer
    let source = "42";
    let tokens = Lexer::new(source).collect::<Vec<_>>();
    let result = parse_expr(tokens, source);
    assert!(result.is_ok());
    // We'll add more assertions as we develop the AST
}

// use miri::parser::parse;
// use miri::ast::*;

// mod shared;

// #[test]
// fn test_variable_declaration() {
//     let source = "x = 10";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_mutable_variable() {
//     let source = "var y = 20";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_typed_variable() {
//     let source = "z int = 30";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_simple_function() {
//     let source = "square(x int) int:
//   x * x";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_function_with_multiple_params() {
//     let source = "add(a int, b int) int:
//   a + b";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_function_with_guard() {
//     let source = "transfer(amount float > 0.0) string:
//   'Transferred: ' + amount";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_if_statement() {
//     let source = "if x > 0:
//   print 'positive'
// else:
//   print 'non-positive'";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_for_loop() {
//     let source = "for i in 0..10:
//   print i";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_while_loop() {
//     let source = "while count > 0:
//   count = count - 1
//   print count";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_do_while_loop() {
//     let source = "do:
//   count = count - 1
//   print count
// while count > 0";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_match_statement() {
//     let source = "match val:
//   0:
//     print 'zero'
//   1 | 2 | 3:
//     print 'low'
//   x if x > 10:
//     print 'large'
//   default:
//     print 'other'";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_use_statement() {
//     let source = "use System.Math";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_selective_import() {
//     let source = "use add, sub from Ops";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_aliased_import() {
//     let source = "use Utils as u";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_array_literal() {
//     let source = "arr = [1, 2, 3]";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_dictionary_literal() {
//     let source = "d = {'a': 1, 'b': 2}";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_array_access() {
//     let source = "x = arr[0]";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_method_call() {
//     let source = "result = arr.map:
//   (x) x * x";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_concurrency() {
//     let source = "html = await fetch('https://example.com')?";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_async_function() {
//     let source = "async fetch(url string) Result<string, Error>:
//   return net.get(url)?";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_parallel_loop() {
//     let source = "|| for item in collection:
//   process(item)";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_actor_spawn() {
//     let source = "counter = spawn Counter.new()
// counter <- inc()
// value = await counter <- get()";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_gpu_function() {
//     let source = "gpu add(a [float], b [float]) [float]:
//   idx = thread_index()
//   return a[idx] + b[idx]";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_symbols() {
//     let source = "status = :active
// if status == :active:
//   print 'Active'";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_error_handling() {
//     let source = "load(path string) Result<string, io::Error>:
//   return fs.read(path)?";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_calc_example() {
//     let source = "add<T numeric>(a T, b T) T: a + b

// sub<T numeric>(a T, b T) T: a - b

// div<T numeric>(a T, b T) T: a / b

// mul<T numeric>(a T, b T) T: a * b";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_hello_world() {
//     let source = "print 'Hello World!'";
//     let result = parse(source);
//     assert!(result.is_ok());
// }

// #[test]
// fn test_simple_calculations() {
//     let source = "use Calc

// calc = Calc.new

// var x = 5
// var y = 10

// var z = calc.add(x, y)
// print('{x} + {y} = {z}', z)

// z = calc.sub(x, y)
// print('{x} - {y} = {z}', z)";
//     let result = parse(source);
//     assert!(result.is_ok());
// }