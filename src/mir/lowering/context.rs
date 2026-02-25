use crate::ast::types::Type;
use crate::error::syntax::Span;
use crate::mir::declaration::Declaration;
use crate::mir::lambda::LambdaInfo;
use crate::mir::module::Import;
use crate::mir::place::{Local, Place};
use crate::mir::{BasicBlock, BasicBlockData, Body, LocalDecl, StatementKind, Terminator};
use std::collections::HashMap;
use std::rc::Rc;

/// Tracks variables introduced in a single scope level.
/// When a scope is exited, these bindings are removed from the variable_map,
/// and any shadowed variables are restored.
#[derive(Debug, Clone)]
pub struct ScopeData {
    pub introduced: Vec<Rc<str>>,
    pub shadowed: HashMap<Rc<str>, Local>,
}

impl ScopeData {
    pub fn new() -> Self {
        Self {
            // Most scopes introduce only a few variables
            introduced: Vec::with_capacity(4),
            shadowed: HashMap::with_capacity(4),
        }
    }
}

impl Default for ScopeData {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone)]
pub struct LoopContext {
    pub break_target: BasicBlock,
    pub continue_target: BasicBlock,
}

pub struct LoweringContext<'a> {
    pub body: Body,
    pub variable_map: HashMap<Rc<str>, Local>, // Map variable names to locals
    pub current_block: BasicBlock,
    pub type_checker: &'a crate::type_checker::TypeChecker,
    /// Stack of scopes for tracking variable visibility
    pub scope_stack: Vec<ScopeData>,
    /// Pool of reusable scopes to reduce allocations
    pub scope_pool: Vec<ScopeData>,
    /// Stack of loops for tracking break/continue targets
    pub loop_stack: Vec<LoopContext>,
    /// Lambda bodies collected during lowering
    pub lambda_bodies: Vec<LambdaInfo>,
    /// Type declarations collected during lowering
    pub declarations: Vec<Declaration>,
    /// Imports collected during lowering
    pub imports: Vec<Import>,
    /// Whether this is a release build (e.g. strip debug names)
    pub is_release: bool,
}

impl<'a> LoweringContext<'a> {
    pub fn new(
        body: Body,
        type_checker: &'a crate::type_checker::TypeChecker,
        is_release: bool,
    ) -> Self {
        let mut ctx = Self {
            body,
            variable_map: HashMap::new(),
            current_block: BasicBlock(0),
            type_checker,
            // Start with a root scope
            scope_stack: vec![ScopeData::new()],
            scope_pool: Vec::with_capacity(8), // Pool for reusing scopes
            loop_stack: Vec::new(),
            lambda_bodies: Vec::new(),
            declarations: Vec::new(),
            imports: Vec::new(),
            is_release,
        };
        // Create the first basic block
        ctx.body.basic_blocks.push(BasicBlockData::new(None));
        ctx
    }

    /// Enter a new loop context
    pub fn enter_loop(&mut self, break_target: BasicBlock, continue_target: BasicBlock) {
        self.loop_stack.push(LoopContext {
            break_target,
            continue_target,
        });
    }

    /// Exit the current loop context
    pub fn exit_loop(&mut self) {
        self.loop_stack.pop();
    }

    /// Get the target for a break statement
    pub fn get_break_target(&self) -> Option<BasicBlock> {
        self.loop_stack.last().map(|loop_ctx| loop_ctx.break_target)
    }

    /// Get the target for a continue statement
    pub fn get_continue_target(&self) -> Option<BasicBlock> {
        self.loop_stack
            .last()
            .map(|loop_ctx| loop_ctx.continue_target)
    }

    /// Enter a new scope. Variables declared in this scope will be tracked
    /// and removed when pop_scope is called.
    pub fn push_scope(&mut self) {
        if let Some(scope) = self.scope_pool.pop() {
            // Reused scope should be empty but have capacity
            self.scope_stack.push(scope);
        } else {
            self.scope_stack.push(ScopeData::new());
        }
    }

    /// Exit the current scope. Removes variables introduced in this scope
    /// and restores any shadowed variables.
    pub fn pop_scope(&mut self, span: Span) {
        if let Some(mut scope) = self.scope_stack.pop() {
            // Remove variables introduced in this scope
            for name in scope.introduced.iter().rev() {
                if let Some(local) = self.variable_map.remove(name) {
                    // Emit StorageDead for variables leaving scope
                    self.push_statement(crate::mir::Statement {
                        kind: StatementKind::StorageDead(Place::new(local)),
                        span,
                    });
                }
            }

            // Restore shadowed variables
            for (name, local) in scope.shadowed.drain() {
                self.variable_map.insert(name, local);
            }

            // Clear introduced for reuse (without shrinking capacity)
            scope.introduced.clear();
            // shadowed is already cleared by drain()

            // Return to pool
            self.scope_pool.push(scope);
        }
    }

    /// Returns the current scope depth (0 = root scope)
    pub fn scope_depth(&self) -> usize {
        self.scope_stack.len().saturating_sub(1)
    }

    pub fn push_local(&mut self, name: String, ty: Type, span: Span) -> Local {
        let mut decl = LocalDecl::new(ty, span);
        let name_rc: Rc<str> = Rc::from(name);

        if !self.is_release {
            decl.name = Some(name_rc.clone());
        }

        decl.is_user_variable = true;
        let local = self.body.new_local(decl);

        // Track in current scope
        if let Some(scope) = self.scope_stack.last_mut() {
            // If this name already exists, save it as shadowed
            if let Some(old_local) = self.variable_map.get(&name_rc) {
                scope.shadowed.insert(name_rc.clone(), *old_local);
            }
            scope.introduced.push(name_rc.clone());
        }

        self.variable_map.insert(name_rc, local);

        // Emit StorageLive for the new local
        self.push_statement(crate::mir::Statement {
            kind: StatementKind::StorageLive(Place::new(local)),
            span,
        });

        local
    }

    /// Register a function parameter (similar to push_local but no StorageLive)
    pub fn push_param(&mut self, name: String, ty: Type, span: Span) -> Local {
        let mut decl = LocalDecl::new(ty, span);
        let name_rc: Rc<str> = Rc::from(name);

        if !self.is_release {
            decl.name = Some(name_rc.clone());
        }

        decl.is_user_variable = true;
        let local = self.body.new_local(decl);

        // Track in current scope
        if let Some(scope) = self.scope_stack.last_mut() {
            // If this name already exists, save it as shadowed
            if let Some(old_local) = self.variable_map.get(&name_rc) {
                scope.shadowed.insert(name_rc.clone(), *old_local);
            }
            scope.introduced.push(name_rc.clone());
        }

        self.variable_map.insert(name_rc, local);
        // implicit StorageLive: parameters are live upon entry

        local
    }

    pub fn push_temp(&mut self, ty: Type, span: Span) -> Local {
        let decl = LocalDecl::new(ty, span);
        self.body.new_local(decl)
    }

    pub fn push_statement(&mut self, statement: crate::mir::Statement) {
        let block = &mut self.body.basic_blocks[self.current_block.0];
        block.statements.push(statement);
    }

    pub fn new_basic_block(&mut self) -> BasicBlock {
        let block = BasicBlockData::new(None);
        self.body.basic_blocks.push(block);
        BasicBlock(self.body.basic_blocks.len() - 1)
    }

    pub fn set_current_block(&mut self, block: BasicBlock) {
        self.current_block = block;
    }

    pub fn set_terminator(&mut self, terminator: Terminator) {
        let block = &mut self.body.basic_blocks[self.current_block.0];
        block.terminator = Some(terminator);
    }
}
