use std::vec;

use crate::lexer::{Lexer, Token, TokenSpan};
use crate::ast::*;


pub struct Parser<'source> {
    lexer: &'source mut Lexer<'source>,
    source: &'source str,
    _lookahead: Option<TokenSpan>,
    _ast_factory: AstFactory,
}

impl<'source> Parser<'source> {
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str, ast_factory: AstFactory) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
            _ast_factory: ast_factory,
        }
    }

    pub fn parse(&mut self) -> Result<Program, &'source str> {
        self._lookahead = self.lexer.next();
        self.program()
    }

    /*
        Program
            : StatementList
            ;
    */
    fn program(&mut self) -> Result<Program, &'source str> {
        let statements = self.statement_list()?;
        Ok(self._ast_factory.create_program(statements))
    }

    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    fn statement_list(&mut self) -> Result<Vec<Statement>, &'source str> {
        let mut statements = vec![self.statement()?];

        // Continue parsing statements until we hit a Dedent or end of input
        while self._lookahead.is_some() && self._lookahead.as_ref().unwrap().0 != Token::Dedent {
            let statement = self.statement()?;
            statements.push(statement);
        }

        Ok(statements)
    }

    /*
        Statement
            : ExpressionStatement
            | BlockStatement
            ;
    */
    fn statement(&mut self) -> Result<Statement, &'source str> {
        let statement = match &self._lookahead {
            Some((Token::Indent, _)) => self.block_statement()?,
            _ => self.expression_statement()?,
        };
        Ok(statement)
    }

    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    fn expression_statement(&mut self) -> Result<Statement, &'source str> {
        let expression = self.expression()?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_expression_statement(expression))
    }

    /*
        BlockStatement
            : Indent OptionalStatementList Dedent
            ;
    */
    fn block_statement(&mut self) -> Result<Statement, &'source str> {
        self.eat_token(&Token::Indent)?;
        let body = match &self._lookahead {
            Some((Token::Dedent, _)) => vec![], // Empty block
            _ => self.statement_list()?,
        };
        self.eat_token(&Token::Dedent)?;
        Ok(self._ast_factory.create_block(body))
    }

    /*
        Expression
            : Literal
            ;
    */
    fn expression(&mut self) -> Result<Expression, &'source str> {
        let expression = self.additive_expression()?;
        Ok(expression)
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    fn additive_expression(&mut self) -> Result<Expression, &'source str> {
        let mut left = self.multiplicative_expression()?;

        while self.lookahead_is_binary_op(is_additive_op) {
            let op = match self.eat_binary_op(is_additive_op) {
                Ok(token) => match token.0 {
                    Token::Plus => BinaryOp::Add,
                    Token::Minus => BinaryOp::Sub,
                    _ => panic!("Unexpected additive operator: {:?}", token.0),
                },
                Err(err) => return Err(err),
            };

            let right = self.multiplicative_expression()?;

            left = self._ast_factory.create_binary_expression(left, op, right);
        }

        Ok(left)
    }


    /*
        MultiplicativeExpression
            : PrimaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR PrimaryExpression
            ;
    */
    fn multiplicative_expression(&mut self) -> Result<Expression, &'source str> {
        let mut left = self.primary_expression()?;

        while self.lookahead_is_binary_op(is_multiplicative_op) {
            let op = match self.eat_binary_op(is_multiplicative_op) {
                Ok(token) => match token.0 {
                    Token::Star => BinaryOp::Mul,
                    Token::Slash => BinaryOp::Div,
                    Token::Percent => BinaryOp::Mod,
                    Token::Pipe => BinaryOp::BitwiseOr,
                    Token::Ampersand => BinaryOp::BitwiseAnd,
                    _ => panic!("Unexpected binary operator: {:?}", token.0),
                },
                Err(err) => return Err(err),
            };

            let right = self.primary_expression()?;

            left = self._ast_factory.create_binary_expression(left, op, right);
        }

        Ok(left)
    }

    /*
        PrimaryExpression
            : Literal
            | ParenthesizedExpression
            ;
    */
    fn primary_expression(&mut self) -> Result<Expression, &'source str> {
        match &self._lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            _ => self.literal_expression(),
        }
    }

    /*
        ParenthesizedExpression
            : '(' Expression ')'
            ;
    */
    fn parenthesized_expression(&mut self) -> Result<Expression, &'source str> {
        self.eat_token(&Token::LParen)?;
        let expression = self.expression()?;
        self.eat_token(&Token::RParen)?;
        Ok(expression)
    }

    /*
        LiteralExpression
            : Literal
            ;
    */
    fn literal_expression(&mut self) -> Result<Expression, &'source str> {
        let literal = self.literal()?;
        Ok(self._ast_factory.create_literal_expression(literal))
    }

    /*
        Literal
            : IntegerLiteral
            : FloatLiteral
            : StringLiteral
            : BooleanLiteral
            : SymbolLiteral
            ;
    */
    fn literal(&mut self) -> Result<Literal, &'source str> {
        match &self._lookahead {
            Some((Token::Int, _)) => self.integer_literal(&Token::Int),
            Some((Token::BinaryNumber, _)) => self.integer_literal(&Token::BinaryNumber),
            Some((Token::HexNumber, _)) => self.integer_literal(&Token::HexNumber),
            Some((Token::OctalNumber, _)) => self.integer_literal(&Token::OctalNumber),
            Some((Token::Float, _)) => self.float_literal(),
            Some((Token::True, _)) => self.boolean_literal(&Token::True),
            Some((Token::False, _)) => self.boolean_literal(&Token::False),
            Some((Token::DoubleQuotedString, _)) => self.string_literal(&Token::DoubleQuotedString),
            Some((Token::SingleQuotedString, _)) => self.string_literal(&Token::SingleQuotedString),
            Some((Token::Symbol, _)) => self.symbol_literal(),
            Some((token, span)) => {
                let token_text = &self.source[span.start..span.end];
                println!("Unexpected token: {:?} with value: '{}'", token, token_text);
                Err("Unsupported literal")
            },
            None => Err("Unexpected end of input"),
        }
    }

    /*
        IntegerLiteral
            : INT
            ;
    */
    fn integer_literal(&mut self, token_type: &Token) -> Result<Literal, &'source str> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                
                // Parse the value based on the token type
                let value = match token_type {
                    Token::Int => str_value.parse::<i128>().map_err(|_| "Invalid integer literal")?,
                    Token::BinaryNumber => {
                        // Strip "0b" prefix and parse as base 2
                        i128::from_str_radix(&str_value[2..], 2).map_err(|_| "Invalid binary literal")?
                    },
                    Token::HexNumber => {
                        // Strip "0x" prefix and parse as base 16
                        i128::from_str_radix(&str_value[2..], 16).map_err(|_| "Invalid hex literal")?
                    },
                    Token::OctalNumber => {
                        // Strip "0o" prefix and parse as base 8
                        i128::from_str_radix(&str_value[2..], 8).map_err(|_| "Invalid octal literal")?
                    },
                    _ => return Err("Unexpected token type for integer literal"),
                };
                
                let literal = match value {
                    v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => self._ast_factory.create_i8_literal(v as i8),
                    v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => self._ast_factory.create_i16_literal(v as i16),
                    v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => self._ast_factory.create_i32_literal(v as i32),
                    v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => self._ast_factory.create_i64_literal(v as i64),
                    v if v >= i128::MIN && v <= i128::MAX => self._ast_factory.create_i128_literal(v),
                    _ => return Err("Integer literal out of range"),
                };
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        FloatLiteral
            : FLOAT
            ;
    */
    fn float_literal(&mut self) -> Result<Literal, &'source str> {
        match self.eat_token(&Token::Float) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                let f32_value = str_value.parse::<f32>().map_err(|_| "Invalid float literal")?;
                let uses_exponent = str_value.contains('e') || str_value.contains('E');
                let f32_str = if uses_exponent {
                    // Count digits after the decimal in the significand (before 'e')
                    let significand = str_value
                        .split(|c| c == 'e' || c == 'E')
                        .next()
                        .unwrap_or("");
                    let decimal_digits = significand
                        .split('.')
                        .nth(1)
                        .unwrap_or("")
                        .len();
                    format!("{:.1$e}", f32_value, decimal_digits)
                } else {
                    let part_after_dot_len = str_value.split('.').nth(1).unwrap_or("").len();
                    format!("{:.1$}", f32_value, part_after_dot_len)
                };                

                fn normalize(s: &str) -> String {
                    let s = s.to_lowercase();
                    if let Some((base, exp)) = s.split_once('e') {
                        let base = base.trim_end_matches('0').trim_end_matches('.');
                        let exp = exp.trim_start_matches('+');
                        format!("{}e{}", base, exp)
                    } else {
                        s.trim_end_matches('0').trim_end_matches('.').to_string()
                    }
                }

                let normalized_input = normalize(str_value);
                let normalized_f32 = normalize(&f32_str);

                // If the f32 representation matches the original string, return as f32
                if normalized_input == normalized_f32 {
                    Ok(self._ast_factory.create_f32_literal(f32_value))
                } else {
                    // Otherwise, parse as f64
                    let f64_value = str_value.parse::<f64>().map_err(|_| "Invalid float literal")?;
                    Ok(self._ast_factory.create_f64_literal(f64_value))
                }
            },
            Err(e) => Err(e),
        }
    }

    /*
        StringLiteral
            : DoubleQuotedString
            : SingleQuotedString
            ;
    */
    fn string_literal(&mut self, token_type: &Token) -> Result<Literal, &'source str> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end - 1]; // Remove quotes
                let literal = self._ast_factory.create_string_literal(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        BooleanLiteral
            : TRUE
            : FALSE
            ;
    */
    fn boolean_literal(&mut self, token_type: &Token) -> Result<Literal, &'source str> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end];
                let literal = match str_value {
                    "true" => self._ast_factory.create_boolean_literal(true),
                    "false" => self._ast_factory.create_boolean_literal(false),
                    _ => return Err("Invalid boolean literal"),
                };
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    /*
        SymbolLiteral
            : SYMBOL
            ;
    */
    fn symbol_literal(&mut self) -> Result<Literal, &'source str> {
        match self.eat_token(&Token::Symbol) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end]; // Remove leading colon
                let literal = self._ast_factory.create_symbol_literal(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    fn eat(&mut self, expected: impl Fn(&Token) -> bool) -> Result<TokenSpan, &'source str> {
        let token = &self._lookahead;

        if token.is_none() {
            if expected(&Token::ExpressionStatementEnd) {
                // Special case for end of expression
                self._lookahead = None;
                return Ok((Token::ExpressionStatementEnd, 0..0));
            }
            return Err("Unexpected end of input");
        }

        match token {
            Some((t, span)) if expected(t) => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next();
                Ok(result)
            },
            _ => Err("Unexpected token type"),
        }
    }

    fn eat_token(&mut self, expected: &Token) -> Result<TokenSpan, &'source str> {
        self.eat(|t| t == expected)
    }

    fn eat_binary_op(&mut self, match_token: fn(&Token) -> bool) -> Result<TokenSpan, &'source str> {
        self.eat(|t| match_token(t))
    }

    fn lookahead_is_binary_op(&self, match_token: fn(&Token) -> bool) -> bool {
        if let Some((token, _)) = &self._lookahead {
            match_token(token)
        } else {
            false
        }
    }
}

fn is_additive_op(token: &Token) -> bool {
    matches!(token, Token::Plus | Token::Minus)
}

fn is_multiplicative_op(token: &Token) -> bool {
    matches!(token, Token::Star | Token::Slash | Token::Percent | Token::Pipe | Token::Ampersand)
}
