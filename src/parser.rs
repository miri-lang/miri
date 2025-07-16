use crate::lexer::{Lexer, Token, TokenSpan};
use crate::ast::*;


pub struct Parser<'source> {
    lexer: &'source mut Lexer<'source>,
    source: &'source str,
    _lookahead: Option<TokenSpan>,
}

impl<'source> Parser<'source> {
    pub fn new(lexer: &'source mut Lexer<'source>, source: &'source str) -> Self {
        Parser {
            lexer,
            source,
            _lookahead: None,
        }
    }

    pub fn parse(&mut self) -> Result<Program, &'source str> {
        self._lookahead = self.lexer.next();
        self.program()
    }

    /*
        Program
            : Literal
            ;
    */
    fn program(&mut self) -> Result<Program, &'source str> {
        let literal = self.literal()?;
        Ok(Program { body: literal })
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
        match self._lookahead {
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
            _ => Err("Unsupported literal"),
        }
    }

    /*
        IntegerLiteral
            : INT
            ;
    */
    fn integer_literal(&mut self, token_type: &Token) -> Result<Literal, &'source str> {
        match self.eat(token_type) {
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
                    v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => Literal::Integer(IntegerLiteral::I8(v as i8)),
                    v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => Literal::Integer(IntegerLiteral::I16(v as i16)),
                    v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => Literal::Integer(IntegerLiteral::I32(v as i32)),
                    v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => Literal::Integer(IntegerLiteral::I64(v as i64)),
                    v if v >= i128::MIN && v <= i128::MAX => Literal::Integer(IntegerLiteral::I128(v)),
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
        match self.eat(&Token::Float) {
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
                    Ok(Literal::Float(FloatLiteral::F32(f32_value)))
                } else {
                    // Otherwise, parse as f64
                    let f64_value = str_value.parse::<f64>().map_err(|_| "Invalid float literal")?;
                    Ok(Literal::Float(FloatLiteral::F64(f64_value)))
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
        match self.eat(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end - 1]; // Remove quotes
                let literal = Literal::String(str_value.to_string());
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
        match self.eat(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end];
                let literal = match str_value {
                    "true" => Literal::Boolean(true),
                    "false" => Literal::Boolean(false),
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
        match self.eat(&Token::Symbol) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end]; // Remove leading colon
                let literal = Literal::Symbol(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    fn eat(&mut self, token_type: &Token) -> Result<TokenSpan, &'source str> {
        let token = &self._lookahead;

        if token.is_none() {
            return Err("Unexpected end of input");
        }

        match token {
            Some((t, span)) if *t == *token_type => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next();
                Ok(result)
            },
            _ => Err("Unexpected token type"),
        }
    }
}
