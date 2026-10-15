// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What a kernel launch requires of its block size and its buffer arguments.
//!
//! The type checker refuses a launch that breaks these rules in the same batch
//! as any other type error; lowering applies the same rules to what only an
//! instantiation reveals, through the same functions, so the two can never
//! disagree about the rule or its wording.

use crate::ast::expression::{Expression, ExpressionKind};
use crate::diagnostics::DiagnosticCode;
use crate::error::syntax::Span;

/// A launch the rules refuse: the code, what to say and where, and the fix.
pub(crate) struct LaunchRefusal {
    pub code: DiagnosticCode,
    pub message: String,
    pub span: Span,
    pub help: String,
}

/// The workgroup size a launch's block argument fixes at compile time.
///
/// It is refused when it is not `Dim3(x, y, z)` of integer literals
/// (MER_TYP_022) or when a dimension is below 1 (MER_TYP_019). `shown`
/// writes an expression as a message quotes it.
pub(crate) fn launch_block_size(
    block: &Expression,
    shown: &dyn Fn(&Expression) -> String,
) -> Result<[u32; 3], LaunchRefusal> {
    let components = match &block.node {
        ExpressionKind::Call(func, args)
            if args.len() == 3
                && matches!(&func.node, ExpressionKind::Identifier(name, _) if name == "Dim3") =>
        {
            args
        }
        _ => {
            return Err(block_size_not_literal(
                block,
                "a `Dim3` of integer literals",
                shown,
            ))
        }
    };
    let mut size = [0u32; 3];
    for (slot, component) in size.iter_mut().zip(components.iter()) {
        let value = integer_literal_value(component)
            .ok_or_else(|| block_size_not_literal(component, "an integer literal", shown))?;
        *slot = u32::try_from(value)
            .ok()
            .filter(|dimension| *dimension > 0)
            .ok_or_else(|| block_dimension_below_one(block, shown))?;
    }
    Ok(size)
}

fn block_size_not_literal(
    found: &Expression,
    expected: &str,
    shown: &dyn Fn(&Expression) -> String,
) -> LaunchRefusal {
    LaunchRefusal {
        code: DiagnosticCode::TypGpuLaunchBlockSizeNotLiteral,
        message: format!(
            "a launch's block size is fixed when its kernel is compiled, \
             but {} is not {expected}",
            shown(found)
        ),
        span: found.span,
        help: "write the block size with integer literals, e.g. `Dim3(16, 16, 1)`".to_string(),
    }
}

fn block_dimension_below_one(
    block: &Expression,
    shown: &dyn Fn(&Expression) -> String,
) -> LaunchRefusal {
    LaunchRefusal {
        code: DiagnosticCode::TypGpuLaunchBlockDimensionsInvalid,
        message: format!(
            "each dimension of a launch's block size must be at least 1, but the block is {}",
            shown(block)
        ),
        span: block.span,
        help: "use 1 for a dimension the kernel does not use, e.g. `Dim3(16, 1, 1)`".to_string(),
    }
}

/// The refusal of a host-resident buffer passed to a `gpu fn`: the kernel
/// reads its buffers from device memory. `name` is the binding passed, or
/// `None` for a value built in the call itself.
pub(crate) fn host_buffer_refusal(name: Option<&str>, span: Span) -> LaunchRefusal {
    let (message, help) = match name {
        Some(name) => (
            format!("a gpu fn reads its buffers from device memory, but `{name}` is host-resident"),
            format!("declare it on the device: `gpu let {name} = ...` (or `gpu var` to write it)"),
        ),
        None => (
            "a gpu fn reads its buffers from device memory, but an array built in the call \
             is host-resident"
                .to_string(),
            "bind it on the device first, e.g. `gpu let buf = [...]`, and pass `buf`".to_string(),
        ),
    };
    LaunchRefusal {
        code: DiagnosticCode::TypGpuFunctionHostBufferMismatch,
        message,
        span,
        help,
    }
}

/// The value of an integer literal, negated ones included; `None` for any
/// other expression, or a literal past the `i128` range.
pub(crate) fn integer_literal_value(expr: &Expression) -> Option<i128> {
    use crate::ast::literal::{IntegerLiteral, Literal};
    use crate::ast::operator::UnaryOp;

    match &expr.node {
        ExpressionKind::Literal(Literal::Integer(literal)) => match *literal {
            IntegerLiteral::I8(v) => Some(v.into()),
            IntegerLiteral::I16(v) => Some(v.into()),
            IntegerLiteral::I32(v) => Some(v.into()),
            IntegerLiteral::I64(v) => Some(v.into()),
            IntegerLiteral::I128(v) => Some(v),
            IntegerLiteral::U8(v) => Some(v.into()),
            IntegerLiteral::U16(v) => Some(v.into()),
            IntegerLiteral::U32(v) => Some(v.into()),
            IntegerLiteral::U64(v) => Some(v.into()),
            IntegerLiteral::U128(v) => i128::try_from(v).ok(),
        },
        ExpressionKind::Unary(UnaryOp::Negate, operand) => {
            integer_literal_value(operand).and_then(i128::checked_neg)
        }
        _ => None,
    }
}

/// `expr` rebuilt from the tree for the forms a launch block holds: a name, a
/// call, an integer literal.
pub(crate) fn rebuilt(expr: &Expression) -> Option<String> {
    match &expr.node {
        ExpressionKind::Identifier(name, _) => Some(name.clone()),
        ExpressionKind::Call(func, args) => {
            let args: Option<Vec<String>> = args.iter().map(rebuilt).collect();
            Some(format!("{}({})", rebuilt(func)?, args?.join(", ")))
        }
        _ => integer_literal_value(expr).map(|value| value.to_string()),
    }
}

impl crate::type_checker::TypeChecker {
    /// Refuses `kernel.launch(grid, block)` whose block size breaks the launch
    /// rules, or whose kernel is a `gpu fn` call handed a host-resident buffer.
    pub(crate) fn check_launch_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        context: &crate::type_checker::context::Context,
    ) {
        let ExpressionKind::Member(receiver, property) = &func.node else {
            return;
        };
        let is_launch =
            matches!(&property.node, ExpressionKind::Identifier(name, _) if name == "launch");
        let on_kernel = self.get_type(receiver.id).is_some_and(|ty| {
            matches!(&ty.kind, crate::ast::types::TypeKind::Custom(name, _) if name == "Kernel")
        });
        let [_, block] = args else {
            return;
        };
        if !is_launch || !on_kernel {
            return;
        }
        if let ExpressionKind::Call(callee, kernel_args) = &receiver.node {
            self.refuse_host_buffers_for_gpu_fn(callee, kernel_args, context);
        }
        if let Err(refusal) = launch_block_size(argument_value(block), &shown) {
            self.report_launch_refusal(refusal);
        }
    }

    /// Refuses each buffer handed to the `gpu fn` `func` names that is not
    /// declared on the device, for a call that is launched. A binding whose
    /// residency is not known here is left to lowering.
    fn refuse_host_buffers_for_gpu_fn(
        &mut self,
        func: &Expression,
        args: &[Expression],
        context: &crate::type_checker::context::Context,
    ) {
        let ExpressionKind::Identifier(callee, _) = &func.node else {
            return;
        };
        let is_gpu_fn = self
            .type_table
            .global_scope
            .get(callee.as_str())
            .is_some_and(|info| info.is_gpu_fn);
        if !is_gpu_fn {
            return;
        }
        let refusals: Vec<LaunchRefusal> = args
            .iter()
            .map(argument_value)
            .filter(|value| {
                self.get_type(value.id)
                    .is_some_and(|ty| is_device_buffer_kind(&ty.kind))
            })
            .filter_map(|value| match &value.node {
                ExpressionKind::Identifier(name, None) => {
                    let residency = context.resolve_info(name)?.residency;
                    (residency == crate::ast::statement::BindingResidency::Host)
                        .then(|| host_buffer_refusal(Some(name), value.span))
                }
                _ => Some(host_buffer_refusal(None, value.span)),
            })
            .collect();
        for refusal in refusals {
            self.report_launch_refusal(refusal);
        }
    }

    fn report_launch_refusal(&mut self, refusal: LaunchRefusal) {
        self.report_error_with_help(refusal.code, refusal.message, refusal.span, refusal.help);
    }
}

/// The value an argument passes, named or not.
fn argument_value(argument: &Expression) -> &Expression {
    match &argument.node {
        ExpressionKind::NamedArgument(_, inner) => inner,
        _ => argument,
    }
}

/// `expr` as the type checker's messages quote it, rebuilt from the tree.
fn shown(expr: &Expression) -> String {
    rebuilt(expr)
        .map(|text| format!("`{text}`"))
        .unwrap_or_else(|| "this value".to_string())
}

/// Whether a value of `kind` is a buffer a `gpu fn` reads from device memory:
/// an array, a list or another built-in collection.
fn is_device_buffer_kind(kind: &crate::ast::types::TypeKind) -> bool {
    use crate::ast::types::{BuiltinCollectionKind, TypeKind};
    match kind {
        TypeKind::Array(..) | TypeKind::List(..) => true,
        TypeKind::Custom(name, _) => BuiltinCollectionKind::from_name(name).is_some(),
        _ => false,
    }
}
