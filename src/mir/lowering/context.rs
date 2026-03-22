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
        mut body: Body,
        type_checker: &'a crate::type_checker::TypeChecker,
        is_release: bool,
    ) -> Self {
        // Pre-compute auto-copy type set from type definitions
        body.auto_copy_types = Self::compute_auto_copy_types(type_checker);
        // Pre-compute field type map for struct/class types (used by Perceus to resolve
        // Field(i) projections without access to the type checker at optimization time).
        body.field_types = Self::compute_field_types(type_checker);

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
            // Only emit StorageDead if the current block is not yet terminated.
            // When a `return` statement sets the terminator first (via
            // `emit_return_cleanup`), the subsequent `pop_scope` from the
            // enclosing `lower_as_return` Block handler must not re-emit
            // StorageDead for variables that were already cleaned up.
            let block_is_live = self.body.basic_blocks[self.current_block.0]
                .terminator
                .is_none();

            // Remove variables introduced in this scope
            for name in scope.introduced.iter().rev() {
                if let Some(local) = self.variable_map.remove(name) {
                    if block_is_live {
                        // Emit StorageDead for variables leaving scope
                        self.push_statement(crate::mir::Statement {
                            kind: StatementKind::StorageDead(Place::new(local)),
                            span,
                        });
                    }
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

    /// Emit `StorageDead` for every named local in every scope, from innermost
    /// to outermost, without modifying the scope stack or `variable_map`.
    ///
    /// Used before a `return` terminator so that managed locals live in
    /// enclosing scopes receive `StorageDead` (and thus a Perceus `DecRef`) on
    /// early-return paths.  The scope stack is intentionally left intact so
    /// that the AST lowering loop for the enclosing block can continue
    /// resolving variables (the current basic block will be dead after Return,
    /// but the MIR lowering loop still runs over remaining AST statements).
    pub fn emit_return_cleanup(&mut self, span: Span) {
        // Simulate pop_scope for each level without mutating the real structures.
        // `effective` tracks what the variable_map would look like after each
        // simulated pop, so shadowed variables are handled correctly.
        //
        // Collect the locals to drop first (avoiding a simultaneous mutable +
        // immutable borrow of `self`), then emit statements in a second pass.
        let mut to_drop: Vec<Local> = Vec::new();
        let mut effective: HashMap<Rc<str>, Local> = self.variable_map.clone();

        for scope in self.scope_stack.iter().rev() {
            // Collect StorageDead targets for each variable introduced by this scope.
            // `effective[name]` is the local bound by this scope at this point.
            for name in scope.introduced.iter().rev() {
                if let Some(&local) = effective.get(name.as_ref()) {
                    to_drop.push(local);
                }
            }

            // Simulate the pop: restore shadowed bindings and remove fresh
            // introductions, so the next (outer) scope sees the right locals.
            for (name, &local) in &scope.shadowed {
                effective.insert(name.clone(), local);
            }
            for name in &scope.introduced {
                if !scope.shadowed.contains_key(name) {
                    effective.remove(name.as_ref());
                }
            }
        }

        for local in to_drop {
            self.push_statement(crate::mir::Statement {
                kind: StatementKind::StorageDead(Place::new(local)),
                span,
            });
        }
    }

    /// Normalize `Custom("Map", Some([k,v]))` → `TypeKind::Map(k,v)` etc. so that
    /// Perceus correctly identifies these locals as managed even when the type
    /// checker returns a `Custom` variant for generic collection constructors.
    fn normalize_collection_type(ty: Type) -> Type {
        use crate::ast::types::TypeKind;
        if let TypeKind::Custom(ref name, Some(ref args)) = ty.kind {
            match (name.as_str(), args.len()) {
                ("Map", 2) => {
                    return Type::new(
                        TypeKind::Map(Box::new(args[0].clone()), Box::new(args[1].clone())),
                        ty.span,
                    );
                }
                ("Set", 1) => {
                    return Type::new(TypeKind::Set(Box::new(args[0].clone())), ty.span);
                }
                ("List", 1) => {
                    return Type::new(TypeKind::List(Box::new(args[0].clone())), ty.span);
                }
                _ => {}
            }
        }
        ty
    }

    pub fn push_local(&mut self, name: String, ty: Type, span: Span) -> Local {
        let ty = Self::normalize_collection_type(ty);
        let mut decl = LocalDecl::new(ty, span);
        let name_rc: Rc<str> = Rc::from(name);

        if !self.is_release {
            decl.name = Some(name_rc.clone());
        }

        decl.is_user_variable = true;
        let local = self.body.new_local(decl);

        // Track in current scope and update variable map (single hash lookup via entry API)
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.introduced.push(name_rc.clone());
        }

        match self.variable_map.entry(name_rc) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old_local = *entry.get();
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.shadowed.insert(entry.key().clone(), old_local);
                }
                entry.insert(local);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(local);
            }
        }

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

        // Track in current scope and update variable map (single hash lookup via entry API)
        if let Some(scope) = self.scope_stack.last_mut() {
            scope.introduced.push(name_rc.clone());
        }

        match self.variable_map.entry(name_rc) {
            std::collections::hash_map::Entry::Occupied(mut entry) => {
                let old_local = *entry.get();
                if let Some(scope) = self.scope_stack.last_mut() {
                    scope.shadowed.insert(entry.key().clone(), old_local);
                }
                entry.insert(local);
            }
            std::collections::hash_map::Entry::Vacant(entry) => {
                entry.insert(local);
            }
        }
        // implicit StorageLive: parameters are live upon entry

        local
    }

    pub fn push_temp(&mut self, ty: Type, span: Span) -> Local {
        let ty = Self::normalize_collection_type(ty);
        let decl = LocalDecl::new(ty, span);
        self.body.new_local(decl)
    }

    /// Returns `true` if `kind` requires reference-count management.
    /// Delegates to the single authority in `crate::mir::rc::is_managed_type`.
    pub fn is_perceus_managed(&self, kind: &crate::ast::types::TypeKind) -> bool {
        crate::mir::rc::is_managed_type(kind, &self.body.auto_copy_types, &self.body.type_params)
    }

    /// Emits `StorageDead` for `local` if it was created at or after `watermark`
    /// and its type is Perceus-managed.
    ///
    /// Used to release temporary locals that were created during expression
    /// lowering and are no longer needed after a call or aggregate assignment.
    /// The Perceus pass will later insert `DecRef` before this statement.
    pub fn emit_temp_drop(
        &mut self,
        local: crate::mir::Local,
        watermark: usize,
        span: crate::error::syntax::Span,
    ) {
        if local.0 < watermark {
            return;
        }
        let kind = self.body.local_decls[local.0].ty.kind.clone();
        if self.is_perceus_managed(&kind) {
            self.push_statement(crate::mir::Statement {
                kind: StatementKind::StorageDead(Place::new(local)),
                span,
            });
        }
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

    /// Returns true if the given type has auto-copy semantics (no RC needed).
    pub fn is_type_auto_copy(&self, ty: &Type) -> bool {
        crate::type_checker::utils::is_auto_copy(
            &ty.kind,
            &self.type_checker.global_type_definitions,
        )
    }

    /// Computes the set of custom type names that qualify as auto-copy.
    fn compute_auto_copy_types(
        type_checker: &crate::type_checker::TypeChecker,
    ) -> std::collections::HashSet<String> {
        let mut auto_copy = std::collections::HashSet::new();
        for name in type_checker.global_type_definitions.keys() {
            let kind = crate::ast::types::TypeKind::Custom(name.clone(), None);
            if crate::type_checker::utils::is_auto_copy(
                &kind,
                &type_checker.global_type_definitions,
            ) {
                auto_copy.insert(name.clone());
            }
        }
        auto_copy
    }

    /// Builds a map from struct/class type names to their ordered field types.
    ///
    /// This is used by the Perceus RC pass to resolve `Field(i)` place projections
    /// and determine whether the projected field type is managed (needs RC).
    ///
    /// For structs, fields are in declaration order.
    /// For classes, fields are in inheritance order (ancestor fields first), matching
    /// the layout produced by `collect_class_fields_all`.
    /// Enum types are excluded: their field layout depends on which variant is active,
    /// so Perceus falls back to checking the LHS local's type for enum field projections.
    fn compute_field_types(
        type_checker: &crate::type_checker::TypeChecker,
    ) -> HashMap<String, Vec<crate::ast::types::Type>> {
        let mut field_types = HashMap::new();

        for (name, def) in &type_checker.global_type_definitions {
            match def {
                crate::type_checker::context::TypeDefinition::Struct(struct_def) => {
                    let types: Vec<_> = struct_def
                        .fields
                        .iter()
                        .map(|(_, ty, _)| ty.clone())
                        .collect();
                    field_types.insert(name.clone(), types);
                }
                crate::type_checker::context::TypeDefinition::Class(class_def) => {
                    let all_fields = crate::type_checker::context::collect_class_fields_all(
                        class_def,
                        &type_checker.global_type_definitions,
                    );
                    let types: Vec<_> = all_fields.iter().map(|(_, fi)| fi.ty.clone()).collect();
                    field_types.insert(name.clone(), types);
                }
                // Enums, aliases, generics, traits: excluded (see doc comment above).
                _ => {}
            }
        }

        field_types
    }
}
