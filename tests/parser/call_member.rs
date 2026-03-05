use miri::lexer::Lexer;
use miri::parser::Parser;

#[test]
fn test_call_member_unexpected_eof_after_less_than() {
    let source = "fn foo() { a < }";
    let mut lexer = Lexer::new(source);
    let mut parser = Parser::new(&mut lexer, source);
    let result = parser.parse();

    // We just want to ensure it doesn't panic.
    // It should ideally return a syntax error about an invalid type declaration.
    assert!(result.is_err());
}
