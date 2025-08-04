// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

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
            | VariableStatement
            | IfStatement
            ;
    */
    fn statement(&mut self) -> Result<Statement, &'source str> {
        let statement = match &self._lookahead {
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement()?,
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            _ => self.expression_statement()?,
        };
        Ok(statement)
    }

    /*
        VariableStatement
            : LetVariableDeclaration
            | VarVariableDeclaration
            ;
    */
    fn variable_statement(&mut self) -> Result<Statement, &'source str> {
        match &self._lookahead {
            Some((Token::Let, _)) => self.let_variable_declaration(),
            Some((Token::Var, _)) => self.var_variable_declaration(),
            _ => Err("Expected variable declaration"),
        }
    }

    /*
        LetVariableDeclaration
            : 'let' VariableDeclarationList EXPRESSION_END
            ;
    */
    fn let_variable_declaration(&mut self) -> Result<Statement, &'source str> {
        self.eat_token(&Token::Let)?;
        let declarations = self.variable_declaration_list(&VariableDeclarationType::Immutable)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_variable_statement(declarations))
    }


    /*
        VarVariableDeclaration
            : 'var' VariableDeclarationList EXPRESSION_END
            ;
    */
    fn var_variable_declaration(&mut self) -> Result<Statement, &'source str> {
        self.eat_token(&Token::Var)?;
        let declarations = self.variable_declaration_list(&VariableDeclarationType::Mutable)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_variable_statement(declarations))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    fn variable_declaration_list(&mut self, declaration_type: &VariableDeclarationType) -> Result<Vec<VariableDeclaration>, &'source str> {
        let mut declarations = vec![self.variable_declaration(declaration_type)?];

        while self.match_lookahead_type(|t| t == &Token::Comma) {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(declaration_type)?);
        }

        Ok(declarations)
    }

    /*
        VariableDeclaration
            : IDENTIFIER
            | IDENTIFIER TYPE
            | IDENTIFIER '=' Expression
            | IDENTIFIER TYPE '=' Expression
            ;
    */
    fn variable_declaration(&mut self, declaration_type: &VariableDeclarationType) -> Result<VariableDeclaration, &'source str> {
        let identifier = self.identifier()?;

        let name;
        if let Expression::Identifier(id) = identifier {
            name = id;
        } else {
            return Err("Expected identifier for variable declaration");
        }

        let typ = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                // If the next token is an identifier, it might be a type
                let token = self.eat_token(&Token::Identifier)?;
                Some(self.source[token.1.start..token.1.end].to_string())
            },
            _ => None,
        };

        let initializer = match &self._lookahead {
            Some((Token::Assign, _)) => {
                self.eat_token(&Token::Assign)?;
                Some(self.expression()?)
            },
            _ => None
        };

        Ok(VariableDeclaration {
            name,
            typ,
            initializer,
            declaration_type: declaration_type.clone(),
        })
    }

    /*
        IfStatement
            : 'if' Expression ':' ExpressionStatement EXPRESSION_END ('else' ExpressionStatement EXPRESSION_END)?
            | 'if' Expression EXPRESSION_END BlockStatement ('else' EXPRESSION_END BlockStatement)?
            ;
    */
    fn if_statement(&mut self, if_statement_type: IfStatementType) -> Result<Statement, &'source str> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }
        let condition = self.expression()?;

        self.try_eat_colon();
        self.try_eat_expression_end();

        let then_block = self.statement()?;

        let else_block = if let Some((Token::Else, _)) = &self._lookahead {
            self.eat_token(&Token::Else)?;
            self.try_eat_colon();
            self.try_eat_expression_end();

            Some(self.statement()?)
        } else {
            None
        };

        Ok(self._ast_factory.create_if_statement(condition, then_block, else_block, if_statement_type))
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
            : AssignmentExpression
            ;
    */
    fn expression(&mut self) -> Result<Expression, &'source str> {
        let expression = self.assignment_expression()?;
        Ok(expression)
    }

    /*
        AssignmentExpression
            : LogicalOrExpression
            | LeftHandSideExpression ASSIGNMENT_OPERATOR AssignmentExpression
            ;
    */
    fn assignment_expression(&mut self) -> Result<Expression, &'source str> {
        let left = self.logical_or_expression()?;

        if !self.lookahead_is_assignment_op() {
            return Ok(left);
        }

        let op = match self.eat_binary_op(is_assignment_op) {
            Ok(token) => match token.0 {
                Token::Assign => AssignmentOp::Assign,
                Token::AssignAdd => AssignmentOp::AssignAdd,
                Token::AssignSub => AssignmentOp::AssignSub,
                Token::AssignMul => AssignmentOp::AssignMul,
                Token::AssignDiv => AssignmentOp::AssignDiv,
                Token::AssignMod => AssignmentOp::AssignMod,
                _ => panic!("Unexpected assignment operator: {:?}", token.0),
            },
            Err(err) => return Err(err),
        };
        let right = self.assignment_expression()?;
        let assignment_expression = self._ast_factory.create_assignment_expression(
            self._ast_factory.create_left_hand_side_expression(left),
            op,
            right,
        );

        Ok(assignment_expression)
    }

    /*
        x > y
        x < y
        x >= y
        x <= y

        RelationalExpression
            : AdditiveExpression
            | AdditiveExpression RELATIONAL_OPERATOR RelationalExpression
            ;
    */
    fn relational_expression(&mut self) -> Result<Expression, &'source str> {
        self.binary_expression(
            Self::additive_expression,
            is_relational_op,
            Self::eat_relational_op
        )
    }

    /*
        x == y
        x != y

        EqualityExpression
            : RelationalExpression EQUALITY_OPERATOR EqualityExpression
            | RelationalExpression
            ;
    */
    fn equality_expression(&mut self) -> Result<Expression, &'source str> {
        self.binary_expression(
            Self::relational_expression,
            is_equality_op,
            Self::eat_equality_op
        )
    }

    /*
        x and y
    
        LogicalAndExpression
            : EqualityExpression AND LogicalAndExpression
            | EqualityExpression
            ;
    */
    fn logical_and_expression(&mut self) -> Result<Expression, &'source str> {
        self.logical_expression(
            Self::equality_expression,
            is_logical_and_op,
            Self::eat_logical_and_op
        )
    }

    /*
        x or y
    
        LogicalOrExpression
            : LogicalAndExpression OR LogicalOrExpression
            | LogicalOrExpression
            ;
    */
    fn logical_or_expression(&mut self) -> Result<Expression, &'source str> {
        self.logical_expression(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op
        )
    }

    /*
        LeftHandSideExpression
            : Identifier
            ;
    */
    fn left_hand_side_expression(&mut self) -> Result<Expression, &'source str> {
        self.identifier()
    }

    /*
        Identifier
            : IDENTIFIER
            ;
    */
    fn identifier(&mut self) -> Result<Expression, &'source str> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                let name = &self.source[token.1.start..token.1.end];                
                Ok(self._ast_factory.create_identifier_expression(name.to_string()))
            },
            _ => Err("Expected identifier"),
        }
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    fn additive_expression(&mut self) -> Result<Expression, &'source str> {
        self.binary_expression(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op
        )
    }

    /*
        MultiplicativeExpression
            : PrimaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR PrimaryExpression
            ;
    */
    fn multiplicative_expression(&mut self) -> Result<Expression, &'source str> {
        self.binary_expression(
            Self::primary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op
        )
    }

    fn generic_binary_expression<F, G, E>(&mut self,
            mut create_branch: F,
            op_predicate: fn(&Token) -> bool,
            mut eat_op: G,
            mut create_expression: E
        ) -> Result<Expression, &'source str> 
    where
        F: FnMut(&mut Self) -> Result<Expression, &'source str>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, &'source str>>,
        E: FnMut(&mut Self, Expression, BinaryOp, Expression) -> Expression,
    {
        let mut left = create_branch(self)?;

        while self.match_lookahead_type(op_predicate) {
            let op = match eat_op(self) {
                Ok(value) => value,
                Err(value) => return value,
            };

            let right = create_branch(self)?;

            left = create_expression(self, left, op, right);
        }

        Ok(left)
    }

    fn binary_expression<F, G>(&mut self,
            create_branch: F,
            op_predicate: fn(&Token) -> bool,
            eat_op: G
        ) -> Result<Expression, &'source str> 
    where
        F: FnMut(&mut Self) -> Result<Expression, &'source str>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, &'source str>>,
    {
        self.generic_binary_expression(
            create_branch,
            op_predicate,
            eat_op,
            |parser, left, op, right| {
                parser._ast_factory.create_binary_expression(left, op, right)
            }
        )
    }

    fn logical_expression<F, G>(&mut self,
            create_branch: F,
            op_predicate: fn(&Token) -> bool,
            eat_op: G
        ) -> Result<Expression, &'source str> 
    where
        F: FnMut(&mut Self) -> Result<Expression, &'source str>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, &'source str>>,
    {
        self.generic_binary_expression(
            create_branch,
            op_predicate,
            eat_op,
            |parser, left, op, right| {
                parser._ast_factory.create_logical_expression(left, op, right)
            }
        )
    }
    
    /*
        PrimaryExpression
            : Literal
            | ParenthesizedExpression
            | LeftHandSideExpression
            ;
    */
    fn primary_expression(&mut self) -> Result<Expression, &'source str> {
        if self.lookahead_is_literal() {
            return self.literal_expression();
        }

        match &self._lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            _ => self.left_hand_side_expression(),
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

    fn match_lookahead_type(&self, match_token: fn(&Token) -> bool) -> bool {
        if let Some((token, _)) = &self._lookahead {
            match_token(token)
        } else {
            false
        }
    }

    fn lookahead_is_assignment_op(&self) -> bool {
        self.match_lookahead_type(is_assignment_op)
    }

    fn lookahead_is_literal(&self) -> bool {
        self.match_lookahead_type(is_literal)
    }

    fn lookahead_is_colon(&self) -> bool {
        self.match_lookahead_type(is_colon)
    }

    fn lookahead_is_expression_end(&self) -> bool {
        self.match_lookahead_type(is_expression_end)
    }

    fn eat_additive_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_additive_op) {
            Ok(token) => match token.0 {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                Token::Pipe => BinaryOp::BitwiseOr,
                Token::Ampersand => BinaryOp::BitwiseAnd,
                Token::Caret => BinaryOp::BitwiseXor,
                _ => panic!("Unexpected additive operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_relational_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_relational_op) {
            Ok(token) => match token.0 {
                Token::LessThan => BinaryOp::LessThan,
                Token::LessThanEqual => BinaryOp::LessThanEqual,
                Token::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                Token::GreaterThan => BinaryOp::GreaterThan,
                _ => panic!("Unexpected relational operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_equality_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_equality_op) {
            Ok(token) => match token.0 {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => panic!("Unexpected equality operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_and_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_logical_and_op) {
            Ok(token) => match token.0 {
                Token::And => BinaryOp::And,
                _ => panic!("Unexpected logical AND operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_or_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_logical_or_op) {
            Ok(token) => match token.0 {
                Token::Or => BinaryOp::Or,
                _ => panic!("Unexpected logical OR operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_multiplicative_op(&mut self) -> Result<BinaryOp, Result<Expression, &'source str>> {
        let op = match self.eat_binary_op(is_multiplicative_op) {
            Ok(token) => match token.0 {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => panic!("Unexpected multiplicative operator: {:?}", token.0),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_expression_end(&mut self) -> Result<TokenSpan, &'source str> {
        self.eat_token(&Token::ExpressionStatementEnd)
    }

    fn try_eat_expression_end(&mut self) {
        if self.lookahead_is_expression_end() {
            let _ = self.eat_expression_end();
        }
    }

    fn eat_colon(&mut self) -> Result<TokenSpan, &'source str> {
        self.eat_token(&Token::Colon)
    }

    fn try_eat_colon(&mut self) {
        if self.lookahead_is_colon() {
            let _ = self.eat_colon();
        }
    }
}

fn is_additive_op(token: &Token) -> bool {
    matches!(token, Token::Plus | Token::Minus | Token::Pipe | Token::Ampersand | Token::Caret)
}

fn is_relational_op(token: &Token) -> bool {
    matches!(token, Token::LessThan | Token::LessThanEqual | Token::GreaterThanEqual | Token::GreaterThan)
}

fn is_equality_op(token: &Token) -> bool {
    matches!(token, Token::Equal | Token::NotEqual)
}

fn is_logical_and_op(token: &Token) -> bool {
    matches!(token, Token::And)
}

fn is_logical_or_op(token: &Token) -> bool {
    matches!(token, Token::Or)
}

fn is_multiplicative_op(token: &Token) -> bool {
    matches!(token, Token::Star | Token::Slash | Token::Percent)
}

fn is_assignment_op(token: &Token) -> bool {
    matches!(token, Token::Assign | Token::AssignAdd | Token::AssignSub | Token::AssignMul | Token::AssignDiv | Token::AssignMod)
}

fn is_literal(token: &Token) -> bool {
    matches!(token, Token::Int | Token::BinaryNumber | Token::HexNumber | Token::OctalNumber | Token::Float | Token::True | Token::False | Token::DoubleQuotedString | Token::SingleQuotedString | Token::Symbol)
}

fn is_colon(token: &Token) -> bool {
    matches!(token, Token::Colon)
}

fn is_expression_end(token: &Token) -> bool {
    matches!(token, Token::ExpressionStatementEnd)
}
