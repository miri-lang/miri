// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Thunks through which the runtime asks a container's elements about each
//! other.
//!
//! The runtime sees an element only as the bytes of its slot, so it cannot
//! order two strings or tell whether two instances of a class are equal. For an
//! element type that answers such a question itself, codegen emits a small
//! exported function taking the two element values and calling the type's own
//! method; the container registers that function's address and calls through
//! it. A List or Array sorts through `__compare_T`, and a Set or Map matches
//! elements and keys through `__equals_T`.

use crate::ast::types::{
    Type, TypeKind, EQUALS_METHOD_NAME, ORDERING_METHOD_NAME, ORDERING_TRAIT_NAME, SELF_TYPE_NAME,
};
use crate::codegen::cranelift::translator::{
    FunctionTranslator, COMPARE_THUNK_PREFIX, EQUALS_THUNK_PREFIX,
};
use crate::error::CodegenError;
use crate::type_checker::context::{class_method_declaration, MethodInfo, TypeDefinition};

use cranelift_codegen::ir::condcodes::IntCC;
use cranelift_codegen::ir::types as cl_types;
use cranelift_codegen::ir::{AbiParam, InstBuilder, Signature, Value};
use cranelift_codegen::isa::{CallConv, TargetIsa};
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};
use cranelift_module::{Linkage, Module};
use cranelift_object::ObjectModule;
use std::collections::HashMap;
use std::sync::Arc;

/// A question a container asks two of its elements, answered by a method the
/// element type declares.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ElementMethod {
    /// Which of the two sorts first: the type's `compare`, a negative, zero or
    /// positive pointer-sized integer.
    Compare,
    /// Whether the two are the same element: the type's own `equals`, a boolean.
    Equals,
}

impl ElementMethod {
    /// Every question, in the order their thunks are emitted.
    pub(crate) const ALL: [ElementMethod; 2] = [ElementMethod::Compare, ElementMethod::Equals];

    fn thunk_prefix(self) -> &'static str {
        match self {
            ElementMethod::Compare => COMPARE_THUNK_PREFIX,
            ElementMethod::Equals => EQUALS_THUNK_PREFIX,
        }
    }

    fn method_name(self) -> &'static str {
        match self {
            ElementMethod::Compare => ORDERING_METHOD_NAME,
            ElementMethod::Equals => EQUALS_METHOD_NAME,
        }
    }

    /// The Cranelift type of the answer, which is the method's return type.
    fn answer_type(self, ptr_type: cl_types::Type) -> cl_types::Type {
        match self {
            ElementMethod::Compare => ptr_type,
            ElementMethod::Equals => cl_types::I8,
        }
    }

    /// Whether elements of `type_name` answer this question themselves.
    ///
    /// Only a class does: a value type's bytes already answer it, and an enum
    /// may be stored as a bare discriminant the thunk's null guard would misread.
    pub(crate) fn is_answered_by(
        self,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> bool {
        let Some(TypeDefinition::Class(_)) = type_definitions.get(type_name) else {
            return false;
        };
        match self {
            ElementMethod::Compare => {
                FunctionTranslator::class_implements(
                    type_name,
                    ORDERING_TRAIT_NAME,
                    type_definitions,
                ) && crate::mir::lowering::dispatch::resolve_inherited_method(
                    type_definitions,
                    type_name,
                    ORDERING_METHOD_NAME,
                )
                .is_some_and(|(_, method)| !method.is_abstract)
            }
            ElementMethod::Equals => {
                class_method_declaration(type_name, EQUALS_METHOD_NAME, type_definitions)
                    .is_some_and(|(declaring, method)| is_element_equality(declaring, method))
            }
        }
    }

    /// The symbol of the method body this question calls for `type_name`.
    ///
    /// Both are resolved through the class chain, the same rule the clone thunk
    /// applies and the one `==` and the ordering operators dispatch by.
    fn method_symbol(
        self,
        type_name: &str,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> String {
        let method_name = self.method_name();
        let owner = crate::mir::lowering::dispatch::resolve_inherited_method(
            type_definitions,
            type_name,
            method_name,
        )
        .map_or_else(|| type_name.to_string(), |(defining, _)| defining);
        format!("{owner}_{method_name}")
    }
}

/// True when `method` is an `equals` a container can call on two elements of
/// `type_name`: it has a body, takes one value of the class itself and answers
/// with a boolean.
///
/// The thunk hands the method two element pointers, so a method declared over
/// some other parameter type would read an element as a value it is not.
fn is_element_equality(type_name: &str, method: &MethodInfo) -> bool {
    let [(_, param_ty)] = method.params.as_slice() else {
        return false;
    };
    let takes_own_type = matches!(
        &param_ty.kind,
        TypeKind::Custom(name, _) if name == type_name || name == SELF_TYPE_NAME
    );
    !method.is_abstract
        && !method.is_static
        && takes_own_type
        && matches!(method.return_type.kind, TypeKind::Boolean)
}

impl<'a> FunctionTranslator<'a> {
    /// Generates `{prefix}{type_name}(a, b)` for a type that answers `method`.
    ///
    /// The two element values are borrowed for the call: a callee owns none of
    /// its parameters, so the container's references survive the question.
    /// `inst_args` selects one instantiation of a generic class, so the thunk
    /// reaches the body compiled for the element's concrete type rather than the
    /// shared one written against the parameter.
    pub(crate) fn generate_element_method_thunk(
        method: ElementMethod,
        module: &mut ObjectModule,
        ctx: &mut cranelift_codegen::Context,
        isa: &Arc<dyn TargetIsa>,
        type_name: &str,
        inst_args: Option<&[Type]>,
        type_definitions: &HashMap<String, TypeDefinition>,
    ) -> Result<(), CodegenError> {
        if !method.is_answered_by(type_name, type_definitions) {
            return Ok(());
        }
        let ptr_type = isa.pointer_type();
        let call_conv = isa.default_call_conv();
        let thunk_name = format!(
            "{}{}",
            method.thunk_prefix(),
            instantiated_symbol(type_name, inst_args)
        );

        let mut sig = Signature::new(call_conv);
        sig.params.push(AbiParam::new(ptr_type));
        sig.params.push(AbiParam::new(ptr_type));
        sig.returns
            .push(AbiParam::new(method.answer_type(ptr_type)));

        let func_id = module
            .declare_function(&thunk_name, Linkage::Export, &sig)
            .map_err(|e| CodegenError::declare_function(thunk_name.clone(), e.to_string()))?;
        ctx.func = cranelift_codegen::ir::Function::with_name_signature(
            cranelift_codegen::ir::UserFuncName::user(0, func_id.as_u32()),
            sig,
        );

        let callee = MethodCallee {
            method,
            symbol: instantiated_symbol(
                &method.method_symbol(type_name, type_definitions),
                inst_args,
            ),
            ptr_type,
            call_conv,
        };
        let mut builder_ctx = FunctionBuilderContext::new();
        emit_element_method_body(module, ctx, &mut builder_ctx, &callee)?;

        module
            .define_function(func_id, ctx)
            .map_err(|e| CodegenError::define_function(thunk_name, e.to_string()))?;
        ctx.clear();
        Ok(())
    }
}

/// The user method a thunk calls, and the ABI it calls it with.
struct MethodCallee {
    method: ElementMethod,
    symbol: String,
    ptr_type: cl_types::Type,
    call_conv: CallConv,
}

/// `base` mangled with an instantiation's type arguments, or `base` itself when
/// there is no instantiation to mangle.
fn instantiated_symbol(base: &str, inst_args: Option<&[Type]>) -> String {
    match inst_args {
        Some(args) => crate::codegen::cranelift::rc::mangle_class_instantiation(base, args),
        None => base.to_string(),
    }
}

/// Emit a thunk body: answer for a null element without calling the user's
/// method, otherwise call it on the two values.
///
/// A null element is not reachable from Miri source — a managed slot always
/// holds a value — but a container the runtime has cleared would otherwise
/// dereference one inside the user's method.
fn emit_element_method_body(
    module: &mut ObjectModule,
    ctx: &mut cranelift_codegen::Context,
    builder_ctx: &mut FunctionBuilderContext,
    callee: &MethodCallee,
) -> Result<(), CodegenError> {
    let mut builder = FunctionBuilder::new(&mut ctx.func, builder_ctx);

    let entry_block = builder.create_block();
    builder.append_block_params_for_function_params(entry_block);
    builder.switch_to_block(entry_block);
    builder.seal_block(entry_block);
    let left = builder.block_params(entry_block)[0];
    let right = builder.block_params(entry_block)[1];

    let call_block = builder.create_block();
    let null_block = builder.create_block();
    let null = builder.ins().iconst(callee.ptr_type, 0);
    let left_is_null = builder.ins().icmp(IntCC::Equal, left, null);
    let right_is_null = builder.ins().icmp(IntCC::Equal, right, null);
    let either_is_null = builder.ins().bor(left_is_null, right_is_null);
    builder
        .ins()
        .brif(either_is_null, null_block, &[], call_block, &[]);

    builder.switch_to_block(null_block);
    builder.seal_block(null_block);
    let null_answer = null_element_answer(&mut builder, callee, [left_is_null, right_is_null]);
    builder.ins().return_(&[null_answer]);

    builder.switch_to_block(call_block);
    builder.seal_block(call_block);
    let answer = emit_user_method_call(module, &mut builder, callee, [left, right])?;
    builder.ins().return_(&[answer]);

    builder.seal_all_blocks();
    builder.finalize();
    Ok(())
}

/// The answer when at least one of the two elements is null.
///
/// Ordering puts a null first — exactly `right_is_null - left_is_null` — and
/// equality holds only when both are null.
fn null_element_answer(
    builder: &mut FunctionBuilder,
    callee: &MethodCallee,
    [left_is_null, right_is_null]: [Value; 2],
) -> Value {
    match callee.method {
        ElementMethod::Compare => {
            let left_rank = builder.ins().uextend(callee.ptr_type, left_is_null);
            let right_rank = builder.ins().uextend(callee.ptr_type, right_is_null);
            builder.ins().isub(right_rank, left_rank)
        }
        ElementMethod::Equals => builder
            .ins()
            .icmp(IntCC::Equal, left_is_null, right_is_null),
    }
}

/// Call the user's compiled method on two element values and hand back its
/// answer. A method always takes the allocator after its declared parameters.
fn emit_user_method_call(
    module: &mut ObjectModule,
    builder: &mut FunctionBuilder,
    callee: &MethodCallee,
    [left, right]: [Value; 2],
) -> Result<Value, CodegenError> {
    let ptr_type = callee.ptr_type;
    let mut sig = Signature::new(callee.call_conv);
    sig.params.push(AbiParam::new(ptr_type)); // self
    sig.params.push(AbiParam::new(ptr_type)); // other
    sig.params.push(AbiParam::new(ptr_type)); // allocator
    sig.returns
        .push(AbiParam::new(callee.method.answer_type(ptr_type)));

    let func_id = module
        .declare_function(&callee.symbol, Linkage::Import, &sig)
        .map_err(|e| CodegenError::declare_function(callee.symbol.clone(), e.to_string()))?;
    let local_fn = module.declare_func_in_func(func_id, builder.func);
    let no_allocator = builder.ins().iconst(ptr_type, 0);
    let call = builder.ins().call(local_fn, &[left, right, no_allocator]);
    Ok(builder.inst_results(call)[0])
}
