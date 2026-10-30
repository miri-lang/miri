// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A bitwise value held as its bytes, in a stack slot of the frame holding it.
//!
//! No program holds a value this way yet: every bitwise value is still
//! counted. These bodies are hand-built with a classifier that holds bitwise
//! values as their bytes, so the storage, copying and call convention a value
//! uses is compiled — with the Cranelift verifier on — and run before any
//! program depends on it.

use miri::ast::literal::{FloatLiteral, IntegerLiteral, Literal};
use miri::ast::types::{Type, TypeKind};
use miri::ast::MemberVisibility;
use miri::codegen::cranelift::{CraneliftBackend, CraneliftOptions};
use miri::codegen::Backend;
use miri::error::syntax::Span;
use miri::mir::place::PlaceElem;
use miri::mir::rc::RcClassifier;
use miri::mir::symbol::Symbol;
use miri::mir::type_facts::TypeFacts;
use miri::mir::{
    AggregateKind, BasicBlock, BasicBlockData, BinOp, Body, Constant, ExecutionModel, Local,
    LocalDecl, Operand, Place, Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::runtime_fns::rt;
use miri::type_checker::context::{StructDefinition, TypeDefinition};
use miri::type_checker::ModuleId;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;

fn span() -> Span {
    Span::default()
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn point() -> TypeKind {
    TypeKind::Custom("P".to_string(), None)
}

fn holder() -> TypeKind {
    TypeKind::Custom("H".to_string(), None)
}

/// A struct definition with the public `fields`.
fn struct_of(fields: [(&str, TypeKind); 2]) -> TypeDefinition {
    let fields = fields
        .into_iter()
        .map(|(name, kind)| (name.to_string(), ty(kind), MemberVisibility::Public))
        .collect();
    TypeDefinition::Struct(StructDefinition {
        fields,
        generics: None,
        module: String::new(),
    })
}

/// `struct P { x int, y float }` and `struct H { k int, p P }`, whose `p`
/// sits at a nonzero offset.
fn definitions() -> HashMap<String, TypeDefinition> {
    HashMap::from([
        (
            "P".to_string(),
            struct_of([("x", TypeKind::Int), ("y", TypeKind::Float)]),
        ),
        (
            "H".to_string(),
            struct_of([("k", TypeKind::Int), ("p", point())]),
        ),
    ])
}

fn place(local: usize) -> Place {
    Place::new(Local(local))
}

fn field(local: usize, index: usize) -> Place {
    Place {
        local: Local(local),
        projection: vec![PlaceElem::Field(index)],
    }
}

fn copy(place: Place) -> Operand {
    Operand::Copy(place)
}

fn int(value: i64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(TypeKind::Int),
        literal: Literal::Integer(IntegerLiteral::I64(value)),
    }))
}

fn float(value: f64) -> Operand {
    Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(TypeKind::Float),
        literal: Literal::Float(FloatLiteral::F64(value.to_bits())),
    }))
}

fn assign(dest: Place, rvalue: Rvalue) -> Statement {
    Statement {
        kind: StatementKind::Assign(dest, rvalue),
        span: span(),
    }
}

fn binary(op: BinOp, lhs: Operand, rhs: Operand) -> Rvalue {
    Rvalue::BinaryOp(op, Box::new(lhs), Box::new(rhs))
}

fn block(statements: Vec<Statement>, kind: TerminatorKind) -> BasicBlockData {
    let mut data = BasicBlockData::new(None);
    data.statements = statements;
    data.terminator = Some(Terminator::new(kind, span()));
    data
}

fn function(name: &str) -> Symbol {
    Symbol::function(&ModuleId::Program, name, &[])
}

fn call(callee: Symbol, args: Vec<Operand>, destination: usize, target: usize) -> TerminatorKind {
    TerminatorKind::Call {
        func: Operand::function(callee, span()),
        args,
        arg_modes: Vec::new(),
        arg_handles: Vec::new(),
        destination: place(destination),
        target: Some(BasicBlock(target)),
    }
}

/// A body over locals of `kinds`, `_0` first, taking `arg_count` of them.
fn body_of(kinds: &[TypeKind], arg_count: usize, blocks: Vec<BasicBlockData>) -> Body {
    let mut body = Body::new(arg_count, span(), ExecutionModel::Cpu);
    for kind in kinds {
        body.new_local(LocalDecl::new(ty(kind.clone()), span()));
    }
    body.basic_blocks.extend(blocks);
    body.rc_classifier = RcClassifier::new(Rc::new(definitions()));
    body
}

/// `fn make(x int, y float) P` builds its result in the caller's slot.
fn make() -> Body {
    let build = Rvalue::Aggregate(
        AggregateKind::Struct(ty(point())),
        vec![copy(place(1)), copy(place(2))],
    );
    body_of(
        &[point(), TypeKind::Int, TypeKind::Float],
        2,
        vec![block(vec![assign(place(0), build)], TerminatorKind::Return)],
    )
}

/// `fn shift(p P) P`: copies `p`, adds one to the copy's `x`, returns the
/// copy.
fn shift() -> Body {
    let statements = vec![
        assign(place(2), Rvalue::Use(copy(place(1)))),
        assign(field(2, 0), binary(BinOp::Add, copy(field(2, 0)), int(1))),
        assign(place(0), Rvalue::Use(copy(place(2)))),
    ];
    body_of(
        &[point(), point(), point()],
        1,
        vec![block(statements, TerminatorKind::Return)],
    )
}

/// `main` exits with `10 * b.x + int(2 * b.y) + a.x + (c.x - 99)`, where
/// `a = make(3, 2.5)`, `b = shift(a)`, then `a.x = 10`, `c = b`, `c.x = 99`.
///
/// Every term reads a different value: 55 holds only when `shift` got a copy
/// of `a`, `b` came back through its return slot, and `c` is a copy of `b`.
fn main_body() -> Body {
    let kinds = [
        TypeKind::Int,
        point(),
        point(),
        point(),
        TypeKind::Int,
        TypeKind::Float,
        TypeKind::Int,
        TypeKind::Int,
    ];
    let statements = vec![
        assign(field(1, 0), Rvalue::Use(int(10))),
        assign(place(3), Rvalue::Use(copy(place(2)))),
        assign(field(3, 0), Rvalue::Use(int(99))),
        assign(place(5), binary(BinOp::Mul, copy(field(2, 1)), float(2.0))),
        assign(
            place(6),
            Rvalue::Cast(Box::new(copy(place(5))), ty(TypeKind::Int)),
        ),
        assign(place(7), binary(BinOp::Mul, copy(field(2, 0)), int(10))),
        assign(place(7), binary(BinOp::Add, copy(place(7)), copy(place(6)))),
        assign(
            place(7),
            binary(BinOp::Add, copy(place(7)), copy(field(1, 0))),
        ),
        assign(place(4), binary(BinOp::Sub, copy(field(3, 0)), int(99))),
        assign(place(0), binary(BinOp::Add, copy(place(7)), copy(place(4)))),
    ];
    body_of(
        &kinds,
        0,
        vec![
            block(
                Vec::new(),
                call(function("make"), vec![int(3), float(2.5)], 1, 1),
            ),
            block(
                Vec::new(),
                call(function("shift"), vec![copy(place(1))], 2, 2),
            ),
            block(statements, TerminatorKind::Return),
        ],
    )
}

fn backend_for(bodies: &[&Body]) -> CraneliftBackend {
    let mut backend = CraneliftBackend::new().expect("host backend");
    let facts = TypeFacts::new(
        definitions(),
        HashMap::new(),
        Default::default(),
        bodies.iter().copied(),
    )
    .expect("type facts");
    backend.set_type_facts(facts);
    backend
}

/// Compile `program`, every body by its link name, with its CLIF listing.
fn compile(program: &[(String, &Body)]) -> Result<(Vec<u8>, String), String> {
    let bodies: Vec<&Body> = program.iter().map(|(_, body)| *body).collect();
    let named: Vec<(&str, &Body)> = program
        .iter()
        .map(|(name, body)| (name.as_str(), *body))
        .collect();
    let options = CraneliftOptions {
        ir_listing: true,
        ..CraneliftOptions::default()
    };
    let artifact = backend_for(&bodies)
        .compile(&named, &options)
        .map_err(|error| error.to_string())?;
    Ok((artifact.bytes, artifact.ir_listing.unwrap_or_default()))
}

fn runtime_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("src/runtime/core/target/release")
}

/// Link `object` against the core runtime and run it, returning its exit
/// status.
fn run(object: &[u8]) -> i32 {
    // Each run links in a directory of its own: tests run side by side.
    static RUNS: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let run = RUNS.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("miri-value-locals-{}-{run}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("scratch directory");
    let object_path = dir.join("program.o");
    let exe_path = dir.join("program");
    std::fs::write(&object_path, object).expect("object written");
    let status = Command::new("cc")
        .arg(&object_path)
        .arg("-o")
        .arg(&exe_path)
        .arg(format!("-L{}", runtime_dir().display()))
        .arg("-lmiri_runtime_core")
        .arg("-lm")
        .status()
        .expect("the linker runs");
    assert!(status.success(), "linking failed: {status}");
    let exit = Command::new(&exe_path).status().expect("the program runs");
    let _ = std::fs::remove_dir_all(&dir);
    exit.code().expect("the program exits")
}

fn program() -> (Body, Body, Body) {
    (make(), shift(), main_body())
}

#[test]
fn a_value_is_built_copied_passed_and_returned_by_its_bytes() {
    let (make, shift, main) = program();
    let named = [
        (function("make").link_name(), &make),
        (function("shift").link_name(), &shift),
        (Symbol::entry().link_name(), &main),
    ];
    let (object, listing) = compile(&named).expect("the program compiles and verifies");
    assert!(
        !listing.contains(rt::BLOCK_ALLOC),
        "a value held as its bytes allocates nothing:\n{listing}"
    );
    assert_eq!(run(&object), 55);
}

/// The CLIF header line of the function linked as `name` in `listing`.
fn header_of<'l>(listing: &'l str, name: &str) -> &'l str {
    let marker = format!("; {name}\n");
    let start = listing.find(&marker).expect("the function is listed") + marker.len();
    listing[start..].lines().next().unwrap_or_default()
}

#[test]
fn a_value_body_is_signed_with_its_members_as_results_and_an_address_parameter() {
    let (make, shift, main) = program();
    let make_name = function("make").link_name();
    let shift_name = function("shift").link_name();
    let named = [
        (make_name.clone(), &make),
        (shift_name.clone(), &shift),
        (Symbol::entry().link_name(), &main),
    ];
    let (_, listing) = compile(&named).expect("the program compiles and verifies");
    let make_header = header_of(&listing, &make_name);
    assert!(
        make_header.contains("(i64, f64) -> i64, f64 "),
        "`make` takes no return slot and returns the value's two members: {make_header}"
    );
    let shift_header = header_of(&listing, &shift_name);
    assert!(
        shift_header.contains("(i64) -> i64, f64 "),
        "`shift` takes its value by address and returns the value's two members: \
         {shift_header}"
    );
}

/// A body whose only statement is `statement`, over a value `_1` and a raw
/// word `_2`.
fn refused(statement: Statement) -> String {
    let body = body_of(
        &[TypeKind::Void, point(), TypeKind::RawPtr],
        0,
        vec![block(vec![statement], TerminatorKind::Return)],
    );
    match compile(&[("refused".to_string(), &body)]) {
        Ok(_) => panic!("the body must be refused"),
        Err(error) => error,
    }
}

#[test]
fn a_value_stored_into_a_word_is_refused() {
    let error = refused(assign(place(2), Rvalue::Use(copy(place(1)))));
    assert!(error.contains("stored into a word"), "{error}");
}

#[test]
fn a_count_adjustment_of_a_value_is_refused() {
    let error = refused(Statement {
        kind: StatementKind::IncRef(place(1)),
        span: span(),
    });
    assert!(
        error.contains("IncRef") && error.contains("no count"),
        "{error}"
    );
}

#[test]
fn a_value_passed_to_a_runtime_function_is_refused() {
    let body = body_of(
        &[TypeKind::Void, point()],
        0,
        vec![
            block(
                Vec::new(),
                TerminatorKind::Call {
                    func: Operand::runtime("miri_rt_example", span()),
                    args: vec![copy(place(1))],
                    arg_modes: Vec::new(),
                    arg_handles: Vec::new(),
                    destination: place(0),
                    target: Some(BasicBlock(1)),
                },
            ),
            block(Vec::new(), TerminatorKind::Return),
        ],
    );
    let error = compile(&[("caller".to_string(), &body)])
        .err()
        .expect("the body must be refused");
    assert!(error.contains("runtime function"), "{error}");
}

/// `main` exits with the payload of an `int?` built present, tested by its
/// tag and opened, plus 10 when an absent one tests absent: 17.
fn optional_main() -> Body {
    let optional = TypeKind::Option(Box::new(ty(TypeKind::Int)));
    let none = Operand::Constant(Box::new(Constant {
        span: span(),
        ty: ty(optional.clone()),
        literal: Literal::None,
    }));
    let present = vec![
        assign(
            place(1),
            Rvalue::Aggregate(AggregateKind::Option(ty(optional.clone())), vec![int(7)]),
        ),
        assign(place(2), Rvalue::Use(none.clone())),
    ];
    let opened = vec![
        assign(place(0), Rvalue::Use(copy(field(1, 0)))),
        assign(place(3), binary(BinOp::Eq, copy(place(2)), none)),
    ];
    let switch =
        |discr: usize, targets: Vec<(u128, usize)>, otherwise: usize| TerminatorKind::SwitchInt {
            discr: copy(place(discr)),
            targets: targets
                .into_iter()
                .map(|(value, target)| (value.into(), BasicBlock(target)))
                .collect(),
            otherwise: BasicBlock(otherwise),
        };
    let absent = vec![assign(
        place(0),
        binary(BinOp::Add, copy(place(0)), int(10)),
    )];
    body_of(
        &[TypeKind::Int, optional.clone(), optional, TypeKind::Boolean],
        0,
        vec![
            block(present, switch(1, vec![(0, 3)], 1)),
            block(opened, switch(3, vec![(1, 2)], 3)),
            block(absent, TerminatorKind::Return),
            block(Vec::new(), TerminatorKind::Return),
        ],
    )
}

#[test]
fn an_optional_is_built_tested_and_opened_by_its_bytes() {
    let main = optional_main();
    let named = [(Symbol::entry().link_name(), &main)];
    let (object, listing) = compile(&named).expect("the program compiles and verifies");
    assert!(
        !listing.contains(rt::BLOCK_ALLOC),
        "an optional held as its bytes allocates nothing:\n{listing}"
    );
    assert_eq!(run(&object), 17);
}

/// `fn bump(p P) int` writes its own parameter: `p.x += 1`, then returns
/// `p.x`.
fn bump() -> Body {
    let statements = vec![
        assign(field(1, 0), binary(BinOp::Add, copy(field(1, 0)), int(1))),
        assign(place(0), Rvalue::Use(copy(field(1, 0)))),
    ];
    body_of(
        &[TypeKind::Int, point()],
        1,
        vec![block(statements, TerminatorKind::Return)],
    )
}

/// `main` exits with `10 * bump(a) + a.x` where `a = make(3, 2.5)`: 43 holds
/// only when `bump` wrote a copy of `a` rather than the caller's bytes.
fn bump_main() -> Body {
    let total = vec![
        assign(place(3), binary(BinOp::Mul, copy(place(2)), int(10))),
        assign(
            place(0),
            binary(BinOp::Add, copy(place(3)), copy(field(1, 0))),
        ),
    ];
    body_of(
        &[TypeKind::Int, point(), TypeKind::Int, TypeKind::Int],
        0,
        vec![
            block(
                Vec::new(),
                call(function("make"), vec![int(3), float(2.5)], 1, 1),
            ),
            block(
                Vec::new(),
                call(function("bump"), vec![copy(place(1))], 2, 2),
            ),
            block(total, TerminatorKind::Return),
        ],
    )
}

#[test]
fn a_callee_writing_its_value_parameter_leaves_the_callers_bytes_alone() {
    let (make, bump, main) = (make(), bump(), bump_main());
    let named = [
        (function("make").link_name(), &make),
        (function("bump").link_name(), &bump),
        (Symbol::entry().link_name(), &main),
    ];
    let (object, _) = compile(&named).expect("the program compiles and verifies");
    assert_eq!(run(&object), 43);
}

/// `main` calls `p = shift(p)` straight into the local it passes, as no
/// lowered body does: the value-argument pass would have copied `p` first.
fn aliased_shift_main() -> Body {
    let read = vec![assign(place(0), Rvalue::Use(copy(field(1, 0))))];
    body_of(
        &[TypeKind::Int, point()],
        0,
        vec![
            block(
                Vec::new(),
                call(function("make"), vec![int(3), float(2.5)], 1, 1),
            ),
            block(
                Vec::new(),
                call(function("shift"), vec![copy(place(1))], 1, 2),
            ),
            block(read, TerminatorKind::Return),
        ],
    )
}

#[test]
fn a_value_argument_aliased_with_the_calls_destination_is_refused() {
    let (make, shift, main) = (make(), shift(), aliased_shift_main());
    let named = [
        (function("make").link_name(), &make),
        (function("shift").link_name(), &shift),
        (Symbol::entry().link_name(), &main),
    ];
    let error = compile(&named).expect_err("the aliased value argument is refused");
    assert!(
        error.contains("is aliased with what the call writes"),
        "{error}"
    );
}

/// `main` exits with `10 * bump(h.p) + h.p.x` where `h = H(k: 7, p: make(3,
/// 2.5))`: `bump` gets the address of `h.p`, eight bytes into `h`, writes its
/// own copy, and 43 holds only when `h.p.x` is still 3. Were `h`'s first byte
/// passed instead, `bump` would read `k` and the exit would be 83.
fn interior_bump_main() -> Body {
    let build = Rvalue::Aggregate(
        AggregateKind::Struct(ty(holder())),
        vec![int(7), copy(place(2))],
    );
    let inner_x = Place {
        local: Local(1),
        projection: vec![PlaceElem::Field(1), PlaceElem::Field(0)],
    };
    let total = vec![
        assign(place(4), binary(BinOp::Mul, copy(place(3)), int(10))),
        assign(place(0), binary(BinOp::Add, copy(place(4)), copy(inner_x))),
    ];
    body_of(
        &[
            TypeKind::Int,
            holder(),
            point(),
            TypeKind::Int,
            TypeKind::Int,
        ],
        0,
        vec![
            block(
                Vec::new(),
                call(function("make"), vec![int(3), float(2.5)], 2, 1),
            ),
            block(
                vec![assign(place(1), build)],
                call(function("bump"), vec![copy(field(1, 1))], 3, 2),
            ),
            block(total, TerminatorKind::Return),
        ],
    )
}

#[test]
fn a_field_passed_from_inside_a_value_local_is_read_in_place() {
    let (make, bump, main) = (make(), bump(), interior_bump_main());
    let named = [
        (function("make").link_name(), &make),
        (function("bump").link_name(), &bump),
        (Symbol::entry().link_name(), &main),
    ];
    let (object, _) = compile(&named).expect("the in-place field argument is accepted");
    assert_eq!(run(&object), 43);
}

/// A tuple of two [`point`]s, which a runtime index can pick between.
fn twin_points() -> TypeKind {
    let member = || miri::ast::expression::Expression {
        id: 0,
        node: miri::ast::expression::ExpressionKind::Type(Box::new(ty(point())), false),
        span: span(),
    };
    TypeKind::Tuple(vec![member(), member()])
}

/// Compile `main` calling `shift` with `arg` into `destination`, as no
/// lowered body does, and hand back the error it is refused with.
fn refused_shift_call(kinds: &[TypeKind], arg: Place, destination: Place) -> String {
    let shift = shift();
    let main = body_of(
        kinds,
        0,
        vec![
            block(
                vec![assign(place(2), Rvalue::Use(int(1)))],
                TerminatorKind::Call {
                    func: Operand::function(function("shift"), span()),
                    args: vec![copy(arg)],
                    arg_modes: Vec::new(),
                    arg_handles: Vec::new(),
                    destination,
                    target: Some(BasicBlock(1)),
                },
            ),
            block(Vec::new(), TerminatorKind::Return),
        ],
    );
    let named = [
        (function("shift").link_name(), &shift),
        (Symbol::entry().link_name(), &main),
    ];
    compile(&named).expect_err("the exposed value argument is refused")
}

#[test]
fn a_field_of_the_calls_destination_reaching_codegen_uncopied_is_refused() {
    let error = refused_shift_call(
        &[TypeKind::Int, holder(), TypeKind::Int],
        field(1, 1),
        field(1, 1),
    );
    assert!(
        error.contains("`_1.1` is aliased with what the call writes"),
        "{error}"
    );
}

#[test]
fn a_member_picked_by_a_runtime_index_reaching_codegen_uncopied_is_refused() {
    let picked = Place {
        local: Local(1),
        projection: vec![PlaceElem::Index(Local(2))],
    };
    let error = refused_shift_call(
        &[TypeKind::Int, twin_points(), TypeKind::Int, point()],
        picked,
        place(3),
    );
    assert!(
        error.contains("reaches the call from inside its holder"),
        "{error}"
    );
}

/// `probe` copies its parameter into `r`, whose address then goes to a
/// call `depth` that recursion keeps out of line, so `r` stays in a slot of
/// the frame; `r.z`, the member the copy stores last, is read between the
/// copy and the call.
const COPY_THEN_FIELD_READ: &str = r#"use system.io

struct Vec3
    x float
    y float
    z float

fn depth(v Vec3, n int) float
    if n == 0
        return v.x
    depth(v, n - 1)

fn probe(a Vec3, n int) float
    var r = a
    let z = r.z
    z + depth(r, n)

fn main()
    println(f"{probe(Vec3(x: 1.0, y: 2.0, z: 3.0), 3)}")
"#;

#[test]
fn a_field_read_after_a_copy_takes_the_stored_value_without_a_load() {
    let (listing, output) = super::slot_promotion::listing_and_output(COPY_THEN_FIELD_READ, "3");
    assert_eq!(output, "4.0\n");
    let probe = super::slot_promotion::function_text(&listing, "miri.probe");
    let loads = probe
        .lines()
        .filter(|line| line.contains(" = load."))
        .count();
    assert_eq!(
        loads, 3,
        "the copy loads each of `a`'s three fields, and `r.z` is taken from \
         the store the copy made rather than loaded again:\n{probe}"
    );
}
