// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2025 Viacheslav Shynkarenko

use std::vec;

use crate::lexer::{token_to_string, Lexer, Token, TokenSpan};
use crate::syntax_error::{Span, SyntaxError, SyntaxErrorKind};
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

    pub fn parse(&mut self) -> Result<Program, SyntaxError> {
        self._lookahead = self.lexer.next().transpose()?;
        self.program()
    }

    /*
        Program
            : StatementList
            ;
    */
    fn program(&mut self) -> Result<Program, SyntaxError> {
        let statements = self.statement_list()?;
        Ok(self._ast_factory.create_program(statements))
    }

    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    fn statement_list(&mut self) -> Result<Vec<Statement>, SyntaxError> {
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
            | IterationStatement
            | EmptyStatement
            ;
    */
    fn statement(&mut self) -> Result<Statement, SyntaxError> {
        if self._lookahead.is_none() {
            return Ok(Statement::Empty);
        }

        let statement = match &self._lookahead {
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) => self.variable_statement()?,
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            Some((Token::While, _)) => self.while_statement(WhileStatementType::While)?,
            Some((Token::Until, _)) => self.while_statement(WhileStatementType::Until)?,
            Some((Token::Forever, _)) => self.while_statement(WhileStatementType::Forever)?,
            Some((Token::For, _)) => self.for_statement()?,
            _ => self.expression_statement()?,
        };
        Ok(statement)
    }

    /*
        VariableStatement
            : 'let' VariableDeclarationList EXPRESSION_END
            | 'var' VariableDeclarationList EXPRESSION_END
            ;
    */
    fn variable_statement(&mut self) -> Result<Statement, SyntaxError> {
        let (token, variable_declaration_type) = match &self._lookahead {
            Some((Token::Let, _)) => (Token::Let, VariableDeclarationType::Immutable),
            Some((Token::Var, _)) => (Token::Var, VariableDeclarationType::Mutable),
            _ => Err(self.error_unexpected_lookahead_token("let or var"))?,
        };

        self.eat_token(&token)?;
        let declarations = self.variable_declaration_list(&variable_declaration_type, true)?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_variable_statement(declarations))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    fn variable_declaration_list(&mut self, declaration_type: &VariableDeclarationType, accept_initializer: bool) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations = vec![self.variable_declaration(declaration_type, accept_initializer)?];

        while self.match_lookahead_type(|t| t == &Token::Comma) {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(declaration_type, accept_initializer)?);
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
    fn variable_declaration(&mut self, declaration_type: &VariableDeclarationType, accept_initializer: bool) -> Result<VariableDeclaration, SyntaxError> {
        let identifier = self.identifier()?;

        let name;
        if let Expression::Identifier(id) = identifier {
            name = id;
        } else {
            return Err(
                self.error_unexpected_token("identifier", format!("{:?}", identifier).as_str())
            );
        }

        let typ = match &self._lookahead {
            Some((Token::Identifier, _)) => {
                // If the next token is an identifier, it might be a type
                let token = self.eat_token(&Token::Identifier)?;
                Some(self.source[token.1.start..token.1.end].to_string())
            },
            _ => None,
        };

        let initializer;
        if accept_initializer {
            initializer = match &self._lookahead {
                Some((Token::Assign, _)) => {
                    self.eat_token(&Token::Assign)?;
                    Some(self.expression()?)
                },
                _ => None
            };
        }
        else {
            initializer = None;
        }

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
    fn if_statement(&mut self, if_statement_type: IfStatementType) -> Result<Statement, SyntaxError> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }
        let condition = self.expression()?;

        self.try_eat_colon();
        self.try_eat_expression_end();

        let then_block = if self._lookahead.is_none() || self.lookahead_is_else() || self.lookahead_is_dedent() {
            Statement::Empty // If there's no else block, we can treat the then block as empty
        } else {
            self.statement()?
        };

        let else_block = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            if self.lookahead_is_colon() {
                let _ = self.eat_colon();
                if self._lookahead.is_none() {
                    None
                } else {
                    if self.lookahead_is_expression_end() {
                        // Empty else branch e.g. `else: // nothing`
                        let _ = self.eat_expression_end();
                        None
                    } else {
                        Some(self.statement()?)
                    }
                }
            } else {
                if self.lookahead_is_expression_end() {
                    let _ = self.eat_expression_end();
                    if self.lookahead_is_indent() {
                        Some(self.block_statement()?)
                    } else {
                        // No valid block after else
                        None
                    }
                } else {
                    Some(self.statement()?)
                }
            }
        } else {
            None
        };

        Ok(self._ast_factory.create_if_statement(condition, then_block, else_block, if_statement_type))
    }

    /*
        WhileStatement
            : 'while' Expression ':' ExpressionStatement EXPRESSION_END
            | 'while' Expression EXPRESSION_END BlockStatement
            | 'until' Expression ':' ExpressionStatement EXPRESSION_END
            | 'until' Expression EXPRESSION_END BlockStatement
            | 'forever' ':' ExpressionStatement EXPRESSION_END
            | 'forever' EXPRESSION_END BlockStatement
            ;
    */
    fn while_statement(&mut self, while_statement_type: WhileStatementType) -> Result<Statement, SyntaxError> {
        let condition;
        if while_statement_type == WhileStatementType::Until {
            self.eat_token(&Token::Until)?;
            condition = self.expression()?;
        } else if while_statement_type == WhileStatementType::Forever {
            self.eat_token(&Token::Forever)?;
            condition = self._ast_factory.create_literal_expression(
                self._ast_factory.create_boolean_literal(true)
            );
        } else {
            self.eat_token(&Token::While)?;
            condition = self.expression()?;
        }

        self.try_eat_colon();
        self.try_eat_expression_end();

        let then_block = if self._lookahead.is_none() || self.lookahead_is_dedent() {
            Statement::Empty // If there's no else block, we can treat the then block as empty
        } else {
            self.statement()?
        };

        Ok(self._ast_factory.create_while_statement(condition, then_block, while_statement_type))
    }

    /*
        RangeBoundaryExpression
            : Identifier
            | StringLiteral
            | IntegerLiteral
            ;
    */
    fn range_boundary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let identifier = self.identifier()?;
                Ok(identifier)
            },
            _ => {
                let err = self.error_unexpected_lookahead_token("an identifier, a string or a number");
                if self.lookahead_is_literal() {
                    let literal = match &self._lookahead {
                        Some((Token::DoubleQuotedString, _)) => self.string_literal(&Token::DoubleQuotedString)?,
                        Some((Token::SingleQuotedString, _)) => self.string_literal(&Token::SingleQuotedString)?,
                        Some((Token::Int, _)) => self.integer_literal(&Token::Int)?,
                        Some((Token::BinaryNumber, _)) => self.integer_literal(&Token::BinaryNumber)?,
                        Some((Token::HexNumber, _)) => self.integer_literal(&Token::HexNumber)?,
                        Some((Token::OctalNumber, _)) => self.integer_literal(&Token::OctalNumber)?,
                        _ => return Err(err),
                    };
                    Ok(self._ast_factory.create_literal_expression(literal))
                } else {
                    Err(err)
                }
            }
        }
    }

    /*
        RangeExpression
            : RangeBoundaryExpression
            | RangeBoundaryExpression .. RangeBoundaryExpression
            | RangeBoundaryExpression ..= RangeBoundaryExpression
            ;
    */
    fn range_expression(&mut self) -> Result<Expression, SyntaxError> {
        let start = self.range_boundary_expression()?;
        let end: Option<Expression>;
        let range_type;

        match &self._lookahead {
            Some((Token::Range, _)) => {
                self.eat_token(&Token::Range)?;
                range_type = RangeExpressionType::Exclusive;
                end = Some(self.range_boundary_expression()?);
            },
            Some((Token::RangeInclusive, _)) => {
                self.eat_token(&Token::RangeInclusive)?;
                range_type = RangeExpressionType::Inclusive;
                end = Some(self.range_boundary_expression()?);
            },
            _ => {
                range_type = RangeExpressionType::IterableObject;
                end = None;
            }
        };

        Ok(self._ast_factory.create_range_expression(start, end, range_type))
    }

    /*
        ForStatement
            : 'for' VariableDeclarationList 'in' RangeExpression ':' ExpressionStatement EXPRESSION_END
            | 'for' VariableDeclarationList 'in' RangeExpression EXPRESSION_END BlockStatement
            ;
    */
    fn for_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::For)?;

        // For loop has immutable variable declarations without initializers
        let variable_declarations = self.variable_declaration_list(
            &VariableDeclarationType::Immutable,
            false
        )?;
        self.eat_token(&Token::In)?;
        let iterable = self.range_expression()?;

        self.try_eat_colon();
        self.try_eat_expression_end();

        let body = if self._lookahead.is_none() || self.lookahead_is_dedent() {
            Statement::Empty // If there's no else block, we can treat the then block as empty
        } else {
            self.statement()?
        };

        Ok(self._ast_factory.create_for_statement(variable_declarations, iterable, body))
    }
    
    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    fn expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;
        self.eat_token(&Token::ExpressionStatementEnd)?;
        Ok(self._ast_factory.create_expression_statement(expression))
    }

    /*
        BlockStatement
            : Indent OptionalStatementList Dedent
            ;
    */
    fn block_statement(&mut self) -> Result<Statement, SyntaxError> {
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
            : ConditionalExpression
            ;
    */
    fn expression(&mut self) -> Result<Expression, SyntaxError> {
        let expression = self.conditional_expression()?;
        Ok(expression)
    }

    /*
        ConditionalExpression
            : AssignmentExpression
            | AssignmentExpression 'if' Expression ('else' Expression)
            | AssignmentExpression 'unless' Expression ('else' Expression)
            ;
    */
    fn conditional_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut expression = self.assignment_expression()?;

        let conditional_token = match &self._lookahead {
            Some((Token::If, _)) => Token::If,
            Some((Token::Unless, _)) => Token::Unless,
            _ => return Ok(expression),
        };

        let if_statement_type = if conditional_token == Token::If {
            IfStatementType::If
        } else {
            IfStatementType::Unless
        };

        self.eat_token(&conditional_token)?;
        let condition = self.expression()?;

        self.try_eat_expression_end();

        let else_branch = if self.match_lookahead_type(|t| t == &Token::Else) {
            self.eat_token(&Token::Else)?;
            Some(self.expression()?)
        } else {
            None
        };

        expression = self._ast_factory.create_conditional_expression(expression, condition, else_branch, if_statement_type);
        
        Ok(expression)
    }

    /*
        AssignmentExpression
            : LogicalOrExpression
            | LeftHandSideExpression ASSIGNMENT_OPERATOR AssignmentExpression
            ;
    */
    fn assignment_expression(&mut self) -> Result<Expression, SyntaxError> {
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
                _ => return Err(
                    self.error_unexpected_operator(token, "=, +=, -=, *=, /=, %=")
                ),
            },
            Err(err) => return Err(err),
        };

        let left = match left {
            Expression::Identifier(name) => LeftHandSideExpression::Identifier(name),
            // Other left-hand side expression types can be added here in the future
            _ => return Err(
                self.error_invalid_left_hand_side_expression()
            ),
        };

        let right = self.assignment_expression()?;

        let assignment_expression = self._ast_factory.create_assignment_expression(
            left,
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
    fn relational_expression(&mut self) -> Result<Expression, SyntaxError> {
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
    fn equality_expression(&mut self) -> Result<Expression, SyntaxError> {
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
    fn logical_and_expression(&mut self) -> Result<Expression, SyntaxError> {
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
    fn logical_or_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.logical_expression(
            Self::logical_and_expression,
            is_logical_or_op,
            Self::eat_logical_or_op
        )
    }

    /*
        LeftHandSideExpression
            : PrimaryExpression
            ;
    */
    fn left_hand_side_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.primary_expression()
    }

    /*
        Identifier
            : IDENTIFIER
            ;
    */
    fn identifier(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Identifier, _)) => {
                let token = self.eat_token(&Token::Identifier)?;
                let name = &self.source[token.1.start..token.1.end];                
                Ok(self._ast_factory.create_identifier_expression(name.to_string()))
            },
            _ => Err(
                self.error_unexpected_lookahead_token("identifier")
            ),
        }
    }

    /*
        AdditiveExpression
            : MultiplicativeExpression
            | AdditiveExpression ADDITIVE_OPERATOR MultiplicativeExpression
            ;
    */
    fn additive_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::multiplicative_expression,
            is_additive_op,
            Self::eat_additive_op
        )
    }

    /*
        MultiplicativeExpression
            : UnaryExpression
            | MultiplicativeExpression MULTIPLICATIVE_OPERATOR UnaryExpression
            ;
    */
    fn multiplicative_expression(&mut self) -> Result<Expression, SyntaxError> {
        self.binary_expression(
            Self::unary_expression,
            is_multiplicative_op,
            Self::eat_multiplicative_op
        )
    }

    fn generic_binary_expression<F, G, E>(&mut self,
            mut create_branch: F,
            op_predicate: fn(&Token) -> bool,
            mut eat_op: G,
            mut create_expression: E
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
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
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
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
        ) -> Result<Expression, SyntaxError> 
    where
        F: FnMut(&mut Self) -> Result<Expression, SyntaxError>,
        G: FnMut(&mut Self) -> Result<BinaryOp, Result<Expression, SyntaxError>>,
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
        UnaryExpression
            : LeftHandSideExpression
            | ADDITIVE_OPERATOR UnaryExpression
            | NOT UnaryExpression
            ;
    */
    fn unary_expression(&mut self) -> Result<Expression, SyntaxError> {
        match &self._lookahead {
            Some((Token::Plus, _)) => self.create_unary_expression(&Token::Plus, UnaryOp::Plus),
            Some((Token::Minus, _)) => self.create_unary_expression(&Token::Minus, UnaryOp::Negate),
            Some((Token::Not, _)) => self.create_unary_expression(&Token::Not, UnaryOp::Not),
            Some((Token::Tilde, _)) => self.create_unary_expression(&Token::Tilde, UnaryOp::BitwiseNot),
            Some((Token::Decrement, _)) => self.create_unary_expression(&Token::Decrement, UnaryOp::Decrement),
            Some((Token::Increment, _)) => self.create_unary_expression(&Token::Increment, UnaryOp::Increment),
            _ => self.left_hand_side_expression(),
        }
    }

    fn create_unary_expression(&mut self, token: &Token, op: UnaryOp) -> Result<Expression, SyntaxError> {
        self.eat_token(token)?;
        let operand = self.unary_expression()?;
        Ok(self._ast_factory.create_unary_expression(op, operand))
    }

    /*
        PrimaryExpression
            : Literal
            | ParenthesizedExpression
            | Identifier
            ;
    */
    fn primary_expression(&mut self) -> Result<Expression, SyntaxError> {
        if self._lookahead.is_none() {
            return Err(self.error_eof());
        }

        if self.lookahead_is_literal() {
            return self.literal_expression();
        }

        match &self._lookahead {
            Some((Token::LParen, _)) => self.parenthesized_expression(),
            Some((Token::Identifier, _)) => self.identifier(),
            _ => Err(
                self.error_unexpected_lookahead_token("literal, parenthesized expression or identifier")
            ),
        }
    }

    /*
        ParenthesizedExpression
            : '(' Expression ')'
            ;
    */
    fn parenthesized_expression(&mut self) -> Result<Expression, SyntaxError> {
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
    fn literal_expression(&mut self) -> Result<Expression, SyntaxError> {
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
    fn literal(&mut self) -> Result<Literal, SyntaxError> {
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
                Err(
                    self.error_unexpected_token_with_span(
                        "a valid literal", 
                        &format!("{:?} with value '{}'", token, token_text),
                        span.clone()
                    )
                )
            },
            None => Err(self.error_eof()),
        }
    }

    /*
        IntegerLiteral
            : INT
            ;
    */
    fn integer_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                
                // Parse the value based on the token type
                let value = match token_type {
                    Token::Int => str_value.parse::<i128>().map_err(|_| {
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidIntegerLiteral,
                            token.1.start..token.1.end
                        )
                    })?,
                    Token::BinaryNumber => {
                        // Strip "0b" prefix and parse as base 2
                        i128::from_str_radix(&str_value[2..], 2).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidBinaryLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    Token::HexNumber => {
                        // Strip "0x" prefix and parse as base 16
                        i128::from_str_radix(&str_value[2..], 16).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidHexLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    Token::OctalNumber => {
                        // Strip "0o" prefix and parse as base 8
                        i128::from_str_radix(&str_value[2..], 8).map_err(|_| {
                            SyntaxError::new(
                                SyntaxErrorKind::InvalidOctalLiteral,
                                token.1.start..token.1.end
                            )
                        })?
                    },
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::UnexpectedToken {
                                expected: "integer literal".to_string(),
                                found: format!("{:?}", token_type),
                            },
                            token.1.start..token.1.end
                        )
                    ),
                };
                
                let literal = match value {
                    v if v >= i8::MIN as i128 && v <= i8::MAX as i128 => self._ast_factory.create_i8_literal(v as i8),
                    v if v >= i16::MIN as i128 && v <= i16::MAX as i128 => self._ast_factory.create_i16_literal(v as i16),
                    v if v >= i32::MIN as i128 && v <= i32::MAX as i128 => self._ast_factory.create_i32_literal(v as i32),
                    v if v >= i64::MIN as i128 && v <= i64::MAX as i128 => self._ast_factory.create_i64_literal(v as i64),
                    v if v >= i128::MIN && v <= i128::MAX => self._ast_factory.create_i128_literal(v),
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::IntegerLiteralOverflow,
                            token.1.start..token.1.end
                        )
                    ),
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
    fn float_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::Float) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end].replace("_", ""); // Remove underscores
                let f32_value = str_value.parse::<f32>().map_err(|_| {
                    SyntaxError::new(
                        SyntaxErrorKind::InvalidFloatLiteral,
                        token.1.start..token.1.end
                    )
                })?;
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
                    let f64_value = str_value.parse::<f64>().map_err(|_| {
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidFloatLiteral,
                            token.1.start..token.1.end
                        )
                    })?;
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
    fn string_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
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
    fn boolean_literal(&mut self, token_type: &Token) -> Result<Literal, SyntaxError> {
        match self.eat_token(token_type) {
            Ok(token) => {
                let str_value = &self.source[token.1.start..token.1.end];
                let literal = match str_value {
                    "true" => self._ast_factory.create_boolean_literal(true),
                    "false" => self._ast_factory.create_boolean_literal(false),
                    _ => return Err(
                        SyntaxError::new(
                            SyntaxErrorKind::InvalidBooleanLiteral,
                            token.1.start..token.1.end
                        )
                    ),
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
    fn symbol_literal(&mut self) -> Result<Literal, SyntaxError> {
        match self.eat_token(&Token::Symbol) {
            Ok(token) => {
                let str_value = &self.source[token.1.start + 1..token.1.end]; // Remove leading colon
                let literal = self._ast_factory.create_symbol_literal(str_value.to_string());
                Ok(literal)
            },
            Err(e) => Err(e),
        }
    }

    fn eat(&mut self, expected: impl Fn(&Token) -> bool) -> Result<TokenSpan, SyntaxError> {
        let token = &self._lookahead;

        match token {
            Some((ref t, ref span)) if expected(t) => {
                let result = (t.clone(), span.clone());
                self._lookahead = self.lexer.next().transpose()?;
                Ok(result)
            },
            Some((found, _)) => {
                Err(
                    SyntaxError::new(
                    SyntaxErrorKind::UnexpectedToken {
                        expected: "".to_string(), // NOTE: This could be improved
                        found: token_to_string(found),
                    },
                    self.source.len()..self.source.len()
                    )
                )
            },
            None => {
                if expected(&Token::ExpressionStatementEnd) {
                    // Special case for end of expression
                    self._lookahead = None;
                    return Ok((Token::ExpressionStatementEnd, 0..0));
                }

                Err(self.error_eof())
            },
        }
    }

    fn eat_token(&mut self, expected: &Token) -> Result<TokenSpan, SyntaxError> {
        self.eat(|t| t == expected)
    }

    fn eat_binary_op(&mut self, match_token: fn(&Token) -> bool) -> Result<TokenSpan, SyntaxError> {
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

    fn lookahead_is_else(&self) -> bool {
        self.match_lookahead_type(is_else)
    }

    fn lookahead_is_indent(&self) -> bool {
        self.match_lookahead_type(is_indent)
    }

    fn lookahead_is_dedent(&self) -> bool {
        self.match_lookahead_type(is_dedent)
    }

    fn lookahead_as_string(&self) -> String {
        self._lookahead.as_ref().map_or("end of file".to_string(), |(t, _)| token_to_string(t))
    }

    fn eat_additive_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_additive_op) {
            Ok(token) => match token.0 {
                Token::Plus => BinaryOp::Add,
                Token::Minus => BinaryOp::Sub,
                Token::Pipe => BinaryOp::BitwiseOr,
                Token::Ampersand => BinaryOp::BitwiseAnd,
                Token::Caret => BinaryOp::BitwiseXor,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "+, -, |, &, ^"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_relational_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_relational_op) {
            Ok(token) => match token.0 {
                Token::LessThan => BinaryOp::LessThan,
                Token::LessThanEqual => BinaryOp::LessThanEqual,
                Token::GreaterThanEqual => BinaryOp::GreaterThanEqual,
                Token::GreaterThan => BinaryOp::GreaterThan,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "<, <=, >, >="))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_equality_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_equality_op) {
            Ok(token) => match token.0 {
                Token::Equal => BinaryOp::Equal,
                Token::NotEqual => BinaryOp::NotEqual,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "=, !="))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_and_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_and_op) {
            Ok(token) => match token.0 {
                Token::And => BinaryOp::And,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "and"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_logical_or_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_logical_or_op) {
            Ok(token) => match token.0 {
                Token::Or => BinaryOp::Or,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "or"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_multiplicative_op(&mut self) -> Result<BinaryOp, Result<Expression, SyntaxError>> {
        let op = match self.eat_binary_op(is_multiplicative_op) {
            Ok(token) => match token.0 {
                Token::Star => BinaryOp::Mul,
                Token::Slash => BinaryOp::Div,
                Token::Percent => BinaryOp::Mod,
                _ => return Err(
                    Err(self.error_unexpected_operator(token, "*, /, %"))
                ),
            },
            Err(err) => return Err(Err(err)),
        };
        Ok(op)
    }

    fn eat_expression_end(&mut self) -> Result<TokenSpan, SyntaxError> {
        self.eat_token(&Token::ExpressionStatementEnd)
    }

    fn try_eat_expression_end(&mut self) {
        if self.lookahead_is_expression_end() {
            let _ = self.eat_expression_end();
        }
    }

    fn eat_colon(&mut self) -> Result<TokenSpan, SyntaxError> {
        self.eat_token(&Token::Colon)
    }

    fn try_eat_colon(&mut self) {
        if self.lookahead_is_colon() {
            let _ = self.eat_colon();
        }
    }

    fn error_unexpected_operator(&self, token: TokenSpan, expected: &str) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: self.lookahead_as_string(),
            },
            token.1.start..token.1.end
        )
    }

    fn error_unexpected_token(&self, expected: &str, found: &str) -> SyntaxError {
        self.error_unexpected_token_with_span(expected, found, self.source.len()..self.source.len())
    }

    fn error_unexpected_token_with_span(&self, expected: &str, found: &str, span: Span) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedToken {
                expected: expected.to_string(),
                found: found.to_string(),
            },
            span
        )
    }

    fn error_unexpected_lookahead_token(&self, expected: &str) -> SyntaxError {
        self.error_unexpected_token(expected, &self.lookahead_as_string())
    }

    fn error_eof(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::UnexpectedEOF,
            self.source.len()..self.source.len()
        )
    }

    fn error_invalid_left_hand_side_expression(&self) -> SyntaxError {
        SyntaxError::new(
            SyntaxErrorKind::InvalidLeftHandSideExpression,
            self.source.len()..self.source.len()
        )
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

fn is_else(token: &Token) -> bool {
    matches!(token, Token::Else)
}

fn is_indent(token: &Token) -> bool {
    matches!(token, Token::Indent)
}

fn is_dedent(token: &Token) -> bool {
    matches!(token, Token::Dedent)
}
