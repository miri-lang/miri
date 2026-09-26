// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::ast::literal::Literal;
use miri::ast::statement::StatementKind as AstStatementKind;
use miri::mir::lambda::LambdaInfo;
use miri::mir::lowering::lower_function;
use miri::mir::{Body, Local, Operand, TerminatorKind};
use miri::pipeline::Pipeline;

/// Lower the top-level function `name` with the implicit allocator parameter,
/// returning its body and every body lowered inside it.
fn lower_with_allocator(source: &str, name: &str) -> (Body, Vec<LambdaInfo>) {
    let result = Pipeline::new().frontend(source).expect("Frontend failed");
    let stmt = result
        .ast
        .body
        .iter()
        .find(|stmt| {
            matches!(&stmt.node, AstStatementKind::FunctionDeclaration(decl) if decl.name == name)
        })
        .expect("function not found");
    lower_function(stmt, &result.type_checker, false, true).expect("Lowering failed")
}

fn local_named(body: &Body, name: &str) -> Local {
    let idx = body
        .local_decls
        .iter()
        .position(|decl| decl.name.as_deref() == Some(name))
        .expect("local not found");
    Local(idx)
}

/// The operands passed by the first call to the global function `callee`.
fn args_of_call_to<'a>(body: &'a Body, callee: &str) -> &'a [Operand] {
    body.basic_blocks
        .iter()
        .filter_map(|block| block.terminator.as_ref())
        .find_map(|term| match &term.kind {
            TerminatorKind::Call {
                func: Operand::Constant(c),
                args,
                ..
            } if matches!(&c.literal, Literal::Identifier(n) if n == callee) => {
                Some(args.as_slice())
            }
            _ => None,
        })
        .expect("call not found")
}

#[test]
fn nested_function_forwards_the_enclosing_allocator_to_its_callees() {
    let (outer, lambdas) = lower_with_allocator(
        r#"
fn base(x int) int
    return x * 3

fn outer(a int) int
    fn helper(n int) int
        return base(n)
    return helper(a)
"#,
        "outer",
    );
    let helper = lambdas
        .iter()
        .find(|info| info.symbol.link_name().contains("helper"))
        .expect("nested body not emitted");

    let args = args_of_call_to(&helper.body, "base");
    assert_eq!(args.len(), 2, "base takes its argument plus the allocator");
    let Operand::Copy(forwarded) = &args[1] else {
        panic!("allocator argument is not a local read: {:?}", args[1]);
    };
    let capture = helper
        .captures
        .iter()
        .find(|cap| cap.lambda_local == forwarded.local)
        .expect("the forwarded allocator is not a capture of the enclosing body");
    assert_eq!(capture.outer_local, local_named(&outer, "allocator"));
}
