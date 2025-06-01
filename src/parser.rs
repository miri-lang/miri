use chumsky::prelude::*;
use crate::lexer::{Token, TokenSpan};
use crate::ast::*;

// Creates a parser for literal (numbers, strings, booleans)
fn literal_parser<'src>(source: &'src str) -> impl Parser<'src, &'src [TokenSpan], Result<Literal, &'src str>> {
    let int = any().filter(|(token, _): &TokenSpan| {
        matches!(token, Token::Int)
    }).map(|(_, span): TokenSpan| {
        let text = &source[span];
        match text.parse() {
            Ok(value) => Ok(Literal::Integer(value)),
            Err(_) => Err("Invalid integer literal")
        }
    });
    
    let float = any().filter(|(token, _): &TokenSpan| {
        matches!(token, Token::Float)
    }).map(|(_, span): TokenSpan| {
        let text = &source[span];
        match text.parse() {
            Ok(value) => Ok(Literal::Float(value)),
            Err(_) => Err("Invalid float literal")
        }
    });

    let string = any().filter(|(token, _): &TokenSpan| {
        matches!(token, Token::SingleQuotedString | Token::DoubleQuotedString)
    }).map(|(_, span): TokenSpan| {
        let text = &source[span];
        Ok(Literal::String(text.to_string()))
    });
    
    let bool_true = any().filter(|(token, _): &TokenSpan| {
        matches!(token, Token::True)
    }).map(|(_, _): TokenSpan| {
        Ok(Literal::Boolean(true))
    });

    let bool_false = any().filter(|(token, _): &TokenSpan| {
        matches!(token, Token::True)
    }).map(|(_, _): TokenSpan| {
        Ok(Literal::Boolean(false))
    });
    
    choice((int, float, string, bool_true, bool_false))
}

// Creates a parser for expressions
pub fn expression_parser<'src>(source: &'src str) -> impl Parser<'src, &'src [TokenSpan], Result<Expression, &'src str>> {
    literal_parser(source)
        .map(|result| {
            match result {
                Ok(literal) => Ok(Expression::Literal(literal)),
                Err(err) => Err(err)
            }
        })
}

// Parse an expression from tokens
pub fn parse<'src>(tokens: &'src [TokenSpan], source: &'src str) -> Result<Expression, &'src str> {
    // Parse using the expression parser
    let parser = expression_parser(source);
    parser.parse(tokens)
}