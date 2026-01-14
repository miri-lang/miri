// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::ast::factory as ast;
use crate::ast::*;
use crate::error::syntax::SyntaxError;
use crate::lexer::Token;

use super::Parser;

impl<'source> Parser<'source> {
    /*
        StatementList
            : Statement
            | StatementList Statement
            ;
    */
    pub(crate) fn statement_list(&mut self) -> Result<Vec<Statement>, SyntaxError> {
        let mut statements = vec![];

        // Keep parsing statements until we hit the end of the file or a dedent.
        while self._lookahead.is_some() && !self.lookahead_is_dedent() {
            statements.push(self.statement()?);
            self.try_eat_expression_end();
        }

        Ok(statements)
    }

    /*
        Statement
            : ExpressionStatement
            | BlockStatement
            | VariableStatement
            | IfStatement
            | WhileStatement
            | ForStatement
            | ForeverStatement
            | FunctionDeclaration
            | ReturnStatement
            | UseStatement
            | TypeStatement
            | BreakStatement
            | ContinueStatement
            | EnumStatement
            | StructStatement
            | EmptyStatement
            ;
    */
    pub(crate) fn statement(&mut self) -> Result<Statement, SyntaxError> {
        if self._lookahead.is_none() {
            return Ok(ast::empty_statement());
        }

        let statement = match &self._lookahead {
            Some((Token::Public, _)) => {
                self.eat_token(&Token::Public)?;
                self.class_member_statement(MemberVisibility::Public)?
            }
            Some((Token::Protected, _)) => {
                self.eat_token(&Token::Protected)?;
                self.class_member_statement(MemberVisibility::Protected)?
            }
            Some((Token::Private, _)) => {
                self.eat_token(&Token::Private)?;
                self.class_member_statement(MemberVisibility::Private)?
            }
            Some((Token::Indent, _)) => self.block_statement()?,
            Some((Token::Let, _)) | Some((Token::Var, _)) => {
                self.variable_statement(MemberVisibility::Public)?
            }
            Some((Token::Shared, _)) => self.shared_variable_statement(MemberVisibility::Public)?,
            Some((Token::If, _)) => self.if_statement(IfStatementType::If)?,
            Some((Token::Unless, _)) => self.if_statement(IfStatementType::Unless)?,
            Some((Token::While, _)) => self.while_statement(WhileStatementType::While)?,
            Some((Token::Until, _)) => self.while_statement(WhileStatementType::Until)?,
            Some((Token::Do, _)) => self.while_statement(WhileStatementType::DoWhile)?,
            Some((Token::Forever, _)) => self.while_statement(WhileStatementType::Forever)?,
            Some((Token::For, _)) => self.for_statement()?,
            Some((Token::Async, _))
            | Some((Token::Fn, _))
            | Some((Token::Gpu, _))
            | Some((Token::Parallel, _)) => self.function_declaration(MemberVisibility::Public)?,
            Some((Token::Return, _)) => self.return_statement()?,
            Some((Token::Use, _)) => self.use_statement()?,
            Some((Token::Type, _)) => self.type_statement(MemberVisibility::Public)?,
            Some((Token::Break, _)) => self.break_statement()?,
            Some((Token::Continue, _)) => self.continue_statement()?,
            Some((Token::Enum, _)) => self.enum_statement(MemberVisibility::Public)?,
            Some((Token::Struct, _)) => self.struct_statement(MemberVisibility::Public)?,
            Some((Token::Class, _)) => self.class_statement(MemberVisibility::Public)?,
            Some((Token::Trait, _)) => self.trait_statement(MemberVisibility::Public)?,
            Some((Token::Abstract, _)) => {
                self.eat_token(&Token::Abstract)?;
                self.abstract_class_statement(MemberVisibility::Public)?
            }
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
    pub(crate) fn variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        let (token, variable_declaration_type) = match &self._lookahead {
            Some((Token::Let, _)) => (Token::Let, VariableDeclarationType::Immutable),
            Some((Token::Var, _)) => (Token::Var, VariableDeclarationType::Mutable),
            _ => Err(self.error_unexpected_lookahead_token("let or var"))?,
        };

        self.eat_token(&token)?;
        let declarations =
            self.variable_declaration_list(&variable_declaration_type, true, false)?;
        Ok(ast::variable_statement(declarations, visibility))
    }

    /*
        SharedVariableStatement
            : 'shared' Identifier Type EXPRESSION_END
            ;
    */
    pub(crate) fn shared_variable_statement(
        &mut self,
        visibility: MemberVisibility,
    ) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Shared)?;

        let name = self.parse_simple_identifier()?;

        // Type is mandatory for shared variables
        let typ_expr = self.type_expression()?;
        let typ = match typ_expr {
            Some(t) => Some(Box::new(t)),
            None => return Err(self.error_unexpected_lookahead_token("type definition")),
        };

        // Shared variables cannot have initializers

        let declaration = VariableDeclaration {
            name,
            typ,
            initializer: None,
            declaration_type: VariableDeclarationType::Mutable,
            is_shared: true,
        };

        self.eat_statement_end()?;

        // Reuse variable_statement node but set is_shared logic inside VariableDeclaration
        Ok(ast::variable_statement(vec![declaration], visibility))
    }

    /*
        VariableDeclarationList
            : VariableDeclaration
            | VariableDeclarationList ',' VariableDeclaration
            ;
    */
    pub(crate) fn variable_declaration_list(
        &mut self,
        declaration_type: &VariableDeclarationType,
        accept_initializer: bool,
        is_shared: bool,
    ) -> Result<Vec<VariableDeclaration>, SyntaxError> {
        let mut declarations =
            vec![self.variable_declaration(declaration_type, accept_initializer, is_shared)?];

        while self.lookahead_is_comma() {
            self.eat_token(&Token::Comma)?;
            declarations.push(self.variable_declaration(
                declaration_type,
                accept_initializer,
                is_shared,
            )?);
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
    pub(crate) fn variable_declaration(
        &mut self,
        declaration_type: &VariableDeclarationType,
        accept_initializer: bool,
        is_shared: bool,
    ) -> Result<VariableDeclaration, SyntaxError> {
        let name = self.parse_simple_identifier()?;

        let typ = self.type_expression()?.map(Box::new);

        let initializer = if accept_initializer {
            match &self._lookahead {
                Some((Token::Assign, _)) => {
                    self.eat_token(&Token::Assign)?;
                    opt_expr(self.expression()?)
                }
                _ => None,
            }
        } else {
            None
        };

        Ok(VariableDeclaration {
            name,
            typ,
            initializer,
            declaration_type: declaration_type.clone(),
            is_shared,
        })
    }

    pub(crate) fn statement_body(&mut self) -> Result<Statement, SyntaxError> {
        if self.lookahead_is_colon() {
            self.eat_token(&Token::Colon)?;

            if self.lookahead_is_expression_end() {
                self.eat_expression_end()?;
                if self.lookahead_is_indent() {
                    return self.block_statement();
                }
                return Ok(ast::empty_statement());
            }

            if self._lookahead.is_none() || self.lookahead_is_dedent() || self.lookahead_is_else() {
                return Ok(ast::empty_statement());
            }
        } else if self.lookahead_is_expression_end() {
            self.eat_expression_end()?;

            if !self.lookahead_is_indent() {
                return Ok(ast::empty_statement());
            }
        } else if self.match_lookahead_type(|t| t == &Token::If || t == &Token::Unless) {
            // To support `else if`
            return self.statement();
        } else {
            return Err(self.error_unexpected_lookahead_token("a colon or an expression end"));
        }

        if self._lookahead.is_some() {
            return self.statement();
        }

        Ok(ast::empty_statement())
    }

    /*
        IfStatement
            : 'if' Expression ':' ExpressionStatement EXPRESSION_END ('else' ExpressionStatement EXPRESSION_END)?
            | 'if' Expression EXPRESSION_END BlockStatement ('else' EXPRESSION_END BlockStatement)?
            ;
    */
    pub(crate) fn if_statement(
        &mut self,
        if_statement_type: IfStatementType,
    ) -> Result<Statement, SyntaxError> {
        if if_statement_type == IfStatementType::Unless {
            self.eat_token(&Token::Unless)?;
        } else {
            self.eat_token(&Token::If)?;
        }
        let condition = self.expression()?;
        let then_block = self.statement_body()?;

        self.try_eat_expression_end();

        let else_block = if self.lookahead_is_else() {
            self.eat_token(&Token::Else)?;
            Some(self.statement_body()?)
        } else {
            None
        };

        if if_statement_type == IfStatementType::Unless {
            Ok(ast::unless_statement(condition, then_block, else_block))
        } else {
            Ok(ast::if_statement(condition, then_block, else_block))
        }
    }

    /*
        WhileStatement
            : 'while' Expression ':' ExpressionStatement EXPRESSION_END
            | 'while' Expression EXPRESSION_END BlockStatement
            | 'until' Expression ':' ExpressionStatement EXPRESSION_END
            | 'until' Expression EXPRESSION_END BlockStatement
            : 'do' ':' ExpressionStatement 'while' Expression EXPRESSION_END
            : 'do' ExpressionStatement 'while' Expression EXPRESSION_END
            | 'forever' ':' ExpressionStatement EXPRESSION_END
            | 'forever' EXPRESSION_END BlockStatement
            ;
    */
    pub(crate) fn while_statement(
        &mut self,
        mut while_statement_type: WhileStatementType,
    ) -> Result<Statement, SyntaxError> {
        let condition;
        let then_block;

        if while_statement_type == WhileStatementType::Until {
            self.eat_token(&Token::Until)?;
            condition = self.expression()?;
            then_block = self.statement_body()?;
        } else if while_statement_type == WhileStatementType::Forever {
            self.eat_token(&Token::Forever)?;
            condition = ast::literal(ast::boolean(true));
            then_block = self.statement_body()?;
        } else if while_statement_type == WhileStatementType::DoWhile {
            self.eat_token(&Token::Do)?;
            then_block = self.statement_body()?;
            match &self._lookahead {
                Some((Token::While, _)) => {
                    self.eat_token(&Token::While)?;
                }
                Some((Token::Until, _)) => {
                    self.eat_token(&Token::Until)?;
                    while_statement_type = WhileStatementType::DoUntil;
                }
                _ => return Err(self.error_unexpected_lookahead_token("while or until")),
            }
            condition = self.expression()?;
        } else {
            self.eat_token(&Token::While)?;
            condition = self.expression()?;
            then_block = self.statement_body()?;
        }

        Ok(ast::while_statement_with_type(
            condition,
            then_block,
            while_statement_type,
        ))
    }

    /*
        ForStatement
            : 'for' VariableDeclarationList 'in' RangeExpression ':' ExpressionStatement EXPRESSION_END
            | 'for' VariableDeclarationList 'in' RangeExpression EXPRESSION_END BlockStatement
            ;
    */
    pub(crate) fn for_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::For)?;

        // For loop has immutable variable declarations without initializers
        let variable_declarations =
            self.variable_declaration_list(&VariableDeclarationType::Immutable, false, false)?;
        self.eat_token(&Token::In)?;
        let iterable_expr = self.range_expression()?;

        let iterable = if let ExpressionKind::Range(_, _, _) = &iterable_expr.node {
            iterable_expr
        } else {
            let span = iterable_expr.span.clone();
            ast::range_with_span(
                iterable_expr,
                None,
                RangeExpressionType::IterableObject,
                span,
            )
        };

        if let ExpressionKind::Range(_, _, range_type) = &iterable.node {
            if *range_type != RangeExpressionType::IterableObject && variable_declarations.len() > 1
            {
                return Err(self.error_unexpected_token(
                    "a single loop variable for a numeric range",
                    &format!("{} variables", variable_declarations.len()),
                ));
            }
        }

        let body = self.statement_body()?;

        Ok(ast::for_statement(variable_declarations, iterable, body))
    }

    /*
        ReturnStatement
            : 'return' Expression EXPRESSION_END
            ;
    */
    pub(crate) fn return_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Return)?;
        let expression = if self.lookahead_is_expression_end() {
            None
        } else {
            opt_expr(self.expression()?)
        };
        self.eat_statement_end()?;
        Ok(ast::return_statement(expression))
    }

    /*
        BreakStatement
            : 'break' EXPRESSION_END
            ;
    */
    pub(crate) fn break_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Break)?;
        self.eat_statement_end()?;
        Ok(ast::break_statement())
    }

    /*
        ContinueStatement
            : 'continue' EXPRESSION_END
            ;
    */
    pub(crate) fn continue_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Continue)?;
        self.eat_statement_end()?;
        Ok(ast::continue_statement())
    }

    /*
        UseStatement
            : 'use' ImportPathExpression ( 'as' Identifier )?
            ;
    */
    pub(crate) fn use_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Use)?;
        let import_path = self.import_path_expression()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            opt_expr(self.identifier()?)
        } else {
            None
        };
        Ok(ast::use_statement(import_path, alias))
    }

    /*
        ImportPathExpression
            : Identifier ('.' Identifier)* ('.' ('*' | '{' ImportList '}'))?
            ;
    */
    pub(crate) fn import_path_expression(&mut self) -> Result<Expression, SyntaxError> {
        let mut segments = vec![];

        if self.match_lookahead_type(|t| t == &Token::System) {
            let (_, span) = self.eat_token(&Token::System)?;
            segments.push(ast::identifier_with_span("system", span));
        } else if self.match_lookahead_type(|t| t == &Token::Local) {
            let (_, span) = self.eat_token(&Token::Local)?;
            segments.push(ast::identifier_with_span("local", span));
        } else {
            segments.push(self.identifier()?);
        }
        let mut kind = ImportPathKind::Simple;

        while self.match_lookahead_type(|t| t == &Token::Dot) {
            self.eat_token(&Token::Dot)?;

            if self.match_lookahead_type(|t| t == &Token::Star) {
                self.eat_token(&Token::Star)?;
                kind = ImportPathKind::Wildcard;
                break; // Wildcard must be the last part of the path
            }

            if self.match_lookahead_type(|t| t == &Token::LBrace) {
                self.eat_token(&Token::LBrace)?;
                let mut multi_imports = vec![self.multi_import_segment()?];
                while self.lookahead_is_comma() {
                    self.eat_token(&Token::Comma)?;
                    if self.match_lookahead_type(|t| t == &Token::RBrace) {
                        break; // Allow trailing comma
                    }
                    multi_imports.push(self.multi_import_segment()?);
                }
                self.eat_token(&Token::RBrace)?;
                kind = ImportPathKind::Multi(multi_imports);
                break; // Multi-import block must be the last part
            }

            segments.push(self.identifier()?);
        }
        Ok(ast::import_path_expression(segments, kind))
    }

    pub(crate) fn multi_import_segment(
        &mut self,
    ) -> Result<(Expression, Option<Box<Expression>>), SyntaxError> {
        let path = self.identifier()?;
        let alias = if self.match_lookahead_type(|t| t == &Token::As) {
            self.eat_token(&Token::As)?;
            Some(Box::new(self.identifier()?))
        } else {
            None
        };
        Ok((path, alias))
    }

    /*
        ExpressionStatement
            : Expression EXPRESSION_END
            ;
    */
    pub(crate) fn expression_statement(&mut self) -> Result<Statement, SyntaxError> {
        let expression = self.expression()?;

        // Special handling for block expressions (like Match) which might consume the Dedent
        // and thus not require a trailing newline if followed by another statement.
        if matches!(expression.node, ExpressionKind::Match(..)) {
            // If we are at a statement start or end of block, we can skip eat_statement_end
            if self.lookahead_is_statement_start()
                || self.lookahead_is_dedent()
                || self._lookahead.is_none()
            {
                return Ok(ast::expression_statement(expression));
            }
        }

        self.eat_statement_end()?;
        Ok(ast::expression_statement(expression))
    }

    /*
        BlockStatement
            : Indent OptionalStatementList Dedent
            ;
    */
    pub(crate) fn block_statement(&mut self) -> Result<Statement, SyntaxError> {
        self.eat_token(&Token::Indent)?;
        let body = match &self._lookahead {
            Some((Token::Dedent, _)) => vec![], // Empty block
            _ => self.statement_list()?,
        };
        self.eat_token(&Token::Dedent)?;
        Ok(ast::block_statement(body))
    }
}
