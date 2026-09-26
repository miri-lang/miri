// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Which vtable slots a program reads: only a slot a reached virtual call
//! reads, through a receiver the instance can stand behind, is filled.

use crate::type_checker::utils::type_checker_result;
use miri::ast::expression::Expression;
use miri::ast::literal::Literal;
use miri::ast::types::{Type, TypeKind, SELF_TYPE_NAME};
use miri::ast::ExpressionKind;
use miri::diagnostics::DiagnosticCode;
use miri::error::lowering::{LoweringError, LoweringErrorKind};
use miri::error::syntax::Span;
use miri::mir::lowering::dispatch_symbols::VtableLayout;
use miri::mir::lowering::instantiation_limits::{
    DEPTH_THROUGH_TRAIT_HELP, MAX_INSTANCE_TYPE_DEPTH, MAX_VALUE_INSTANCES_PER_CLASS, VALUE_HELP,
};
use miri::mir::lowering::vtable_demand::{DemandTables, FilledSlot, VtableDemand};
use miri::mir::{
    AggregateKind, BasicBlockData, Body, Constant, ExecutionModel, LocalDecl, Operand, Place,
    Rvalue, Statement, StatementKind, Terminator, TerminatorKind,
};
use miri::pipeline::PipelineResult;

const SOURCE: &str = "
trait Op<T>
    fn keep(a T, b T) T
    fn size() int

class Impl<T> implements Op<T>
    fn keep(a T, b T) T
        return b
    fn size() int
        return 1

class Wrap<T>
    v T
    fn init(v T)
        self.v = v

class Buf<T, Size> implements Op<T>
    fn keep(a T, b T) T
        return b
    fn size() int
        return 1
";

fn span() -> Span {
    Span::new(0, 0)
}

fn ty(kind: TypeKind) -> Type {
    Type::new(kind, span())
}

fn argument(kind: TypeKind) -> Expression {
    Expression {
        id: 0,
        span: span(),
        node: ExpressionKind::Type(Box::new(ty(kind)), false),
    }
}

/// `name<args>`, the arguments written as types.
fn generic(name: &str, args: Vec<TypeKind>) -> TypeKind {
    TypeKind::Custom(
        name.to_string(),
        Some(args.into_iter().map(argument).collect()),
    )
}

/// A body building one instance of each of `constructed`, naming each of
/// `calls`, and ending in a virtual call when `dispatch` gives its receiver's
/// type and the slot it reads.
struct BodySketch {
    constructed: Vec<TypeKind>,
    calls: Vec<&'static str>,
    dispatch: Option<(TypeKind, usize)>,
    type_params: Vec<&'static str>,
    has_self: bool,
}

impl BodySketch {
    fn new() -> Self {
        Self {
            constructed: Vec::new(),
            calls: Vec::new(),
            dispatch: None,
            type_params: Vec::new(),
            has_self: false,
        }
    }

    /// Give the body a `self` local spelled `Self`, as a struct method's
    /// explicit `self` parameter is.
    fn receives_self(mut self) -> Self {
        self.has_self = true;
        self
    }

    fn constructs(mut self, kind: TypeKind) -> Self {
        self.constructed.push(kind);
        self
    }

    fn calls(mut self, symbol: &'static str) -> Self {
        self.calls.push(symbol);
        self
    }

    fn dispatches(mut self, receiver: TypeKind, slot: usize) -> Self {
        self.dispatch = Some((receiver, slot));
        self
    }

    fn with_parameter(mut self, name: &'static str) -> Self {
        self.type_params.push(name);
        self
    }

    fn body(self) -> Body {
        let mut body = Body::new(0, span(), ExecutionModel::Cpu);
        body.type_params = self.type_params.iter().map(|p| p.to_string()).collect();
        if self.has_self {
            let self_ty = TypeKind::Custom(SELF_TYPE_NAME.to_string(), None);
            body.new_local(LocalDecl::new(ty(self_ty), span()));
        }
        let mut block = BasicBlockData::new(None);
        for kind in self.constructed {
            let local = body.new_local(LocalDecl::new(ty(kind.clone()), span()));
            block.statements.push(Statement {
                kind: StatementKind::Assign(
                    Place::new(local),
                    Rvalue::Aggregate(AggregateKind::Class(ty(kind)), Vec::new()),
                ),
                span: span(),
            });
        }
        for symbol in self.calls {
            let local = body.new_local(LocalDecl::new(ty(TypeKind::Int), span()));
            let callee = Operand::Constant(Box::new(Constant {
                span: span(),
                ty: ty(TypeKind::Identifier),
                literal: Literal::Identifier(symbol.to_string()),
            }));
            block.statements.push(Statement {
                kind: StatementKind::Assign(Place::new(local), Rvalue::Use(callee)),
                span: span(),
            });
        }
        if let Some((receiver, slot)) = self.dispatch {
            let receiver_local = body.new_local(LocalDecl::new(ty(receiver), span()));
            let destination = body.new_local(LocalDecl::new(ty(TypeKind::Int), span()));
            block.terminator = Some(Terminator::new(
                TerminatorKind::VirtualCall {
                    vtable_slot: slot,
                    args: vec![Operand::Copy(Place::new(receiver_local))],
                    out_args: Vec::new(),
                    destination: Place::new(destination),
                    target: None,
                },
                span(),
            ));
        }
        body.basic_blocks.push(block);
        body
    }
}

struct Fixture {
    result: PipelineResult,
    layout: VtableLayout,
}

impl Fixture {
    fn new() -> Self {
        let result = type_checker_result(SOURCE);
        let layout = VtableLayout::of(result.type_checker.type_definitions());
        Self { result, layout }
    }

    fn tables(&self) -> DemandTables<'_> {
        DemandTables {
            type_checker: &self.result.type_checker,
            layout: &self.layout,
        }
    }

    fn slot(&self, method: &str) -> usize {
        self.layout
            .slot(method)
            .expect("a trait method takes a slot")
    }

    fn record(&self, demand: &mut VtableDemand, bodies: &[(&str, Body)]) {
        self.try_record(demand, bodies)
            .expect("the demand accepts every body recorded");
    }

    fn try_record(
        &self,
        demand: &mut VtableDemand,
        bodies: &[(&str, Body)],
    ) -> Result<(), LoweringError> {
        demand.record(
            bodies.iter().map(|(symbol, body)| (*symbol, body)),
            self.tables(),
        )
    }
}

fn filled(demand: &VtableDemand, vtable: &str) -> Vec<FilledSlot> {
    demand.fills().slots(vtable).to_vec()
}

fn impl_at(kind: TypeKind) -> TypeKind {
    generic("Impl", vec![kind])
}

fn op_at(kind: TypeKind) -> TypeKind {
    generic("Op", vec![kind])
}

/// A slot the class declares that no reached call reads stays unfilled, and
/// the one a call reads names the instantiation's body.
#[test]
fn a_declared_slot_no_call_reads_stays_unfilled() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let main = BodySketch::new()
        .constructs(impl_at(TypeKind::String))
        .dispatches(op_at(TypeKind::String), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    assert_eq!(
        filled(&demand, "__vtable_Impl__String"),
        vec![FilledSlot {
            slot: keep,
            method: "keep".to_string(),
            symbol: "Impl_keep__String".to_string(),
        }],
    );
    assert_eq!(demand.take_named_slot_symbols(), vec!["Impl_keep__String"]);
}

/// The instance and the call reading its slot can come from bodies recorded
/// apart; the slot is filled once both are, and recording nothing new after
/// that names nothing.
#[test]
fn a_slot_is_filled_once_its_instance_and_its_call_are_both_recorded() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let mut demand = VtableDemand::default();
    let builder = BodySketch::new().constructs(impl_at(TypeKind::Int)).body();
    fixture.record(&mut demand, &[("build", builder)]);
    assert!(demand.take_named_slot_symbols().is_empty());

    let caller = BodySketch::new()
        .dispatches(op_at(TypeKind::Int), keep)
        .body();
    fixture.record(&mut demand, &[("call", caller)]);
    assert_eq!(demand.take_named_slot_symbols(), vec!["Impl_keep__int"]);

    let callee = BodySketch::new().body();
    fixture.record(&mut demand, &[("Impl_keep__int", callee)]);
    assert!(demand.take_named_slot_symbols().is_empty());
    assert_eq!(filled(&demand, "__vtable_Impl__int").len(), 1);
}

/// A call through `Op<int>` fills the `int` instance's slot and leaves the
/// `String` instance's alone, though both answer `keep`.
#[test]
fn a_slot_is_filled_only_where_the_receiver_arguments_agree() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let main = BodySketch::new()
        .constructs(impl_at(TypeKind::String))
        .constructs(impl_at(TypeKind::Int))
        .dispatches(op_at(TypeKind::Int), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    assert_eq!(filled(&demand, "__vtable_Impl__int").len(), 1);
    assert!(filled(&demand, "__vtable_Impl__String").is_empty());
}

/// A receiver whose argument is still open can reach any instance.
#[test]
fn a_receiver_at_an_open_argument_reads_every_instance() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let main = BodySketch::new()
        .constructs(impl_at(TypeKind::String))
        .constructs(impl_at(TypeKind::Int))
        .dispatches(TypeKind::Custom("Op".to_string(), None), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    assert_eq!(filled(&demand, "__vtable_Impl__int").len(), 1);
    assert_eq!(filled(&demand, "__vtable_Impl__String").len(), 1);
}

/// A shared generic body builds its instance at an open argument; it counts
/// only once a reached body calls it.
#[test]
fn a_shared_generic_body_counts_only_once_reached() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let open_param = TypeKind::Custom("T".to_string(), None);
    let shared = BodySketch::new()
        .constructs(impl_at(open_param))
        .with_parameter("T")
        .body();
    let main = BodySketch::new()
        .dispatches(op_at(TypeKind::String), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("make", shared), ("main", main)]);
    assert_eq!(demand.vtable_symbols().count(), 0);

    let caller = BodySketch::new().calls("make").body();
    fixture.record(&mut demand, &[("run", caller)]);
    assert_eq!(
        demand.vtable_symbols().collect::<Vec<_>>(),
        vec!["__vtable_Impl"]
    );
    assert_eq!(demand.take_named_slot_symbols(), vec!["Impl_keep"]);
}

/// A body reached through `Impl<int>`'s slot that builds `Impl<Wrap<int>>`
/// grows past the instance it was reached through; the grown instance's slot
/// still names the body compiled for its own arguments, never the class's
/// shared one.
#[test]
fn an_instance_growing_past_the_one_it_was_reached_through_is_compiled_for_its_own_arguments() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let main = BodySketch::new()
        .constructs(impl_at(TypeKind::Int))
        .dispatches(op_at(TypeKind::Int), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    assert_eq!(demand.take_named_slot_symbols(), vec!["Impl_keep__int"]);

    let wrapped = generic("Wrap", vec![TypeKind::Int]);
    let body = BodySketch::new()
        .constructs(impl_at(wrapped.clone()))
        .dispatches(op_at(wrapped), keep)
        .body();
    fixture.record(&mut demand, &[("Impl_keep__int", body)]);
    assert_eq!(
        demand.take_named_slot_symbols(),
        vec!["Impl_keep__Wrap_int"]
    );
    assert_eq!(
        filled(&demand, "__vtable_Impl__Wrap_int")[0].symbol,
        "Impl_keep__Wrap_int"
    );
}

/// `n` layers of `Wrap` around `int`.
fn wrapped_int(layers: usize) -> TypeKind {
    (0..layers).fold(TypeKind::Int, |inner, _| generic("Wrap", vec![inner]))
}

/// Record the body each slot the last round named, as the body the class's
/// `keep` compiles to at `Impl<Wrap^n<int>>`: it builds and calls the class
/// one `Wrap` deeper. Returns the first refusal, with the number of `Wrap`
/// layers the refused instance holds.
fn grow_until_refused(fixture: &Fixture, demand: &mut VtableDemand) -> (usize, LoweringError) {
    let keep = fixture.slot("keep");
    for layers in 1..64 {
        let named = demand.take_named_slot_symbols();
        assert_eq!(named.len(), 1, "one slot is named per level");
        let body = BodySketch::new()
            .constructs(impl_at(wrapped_int(layers)))
            .dispatches(op_at(wrapped_int(layers)), keep)
            .body();
        if let Err(error) = fixture.try_record(demand, &[(named[0].as_str(), body)]) {
            return (layers, error);
        }
    }
    panic!("an instance growing on every level was never refused");
}

/// The message, help and notes of a coded refusal.
fn refusal_text(error: &LoweringError) -> (String, String, Vec<String>) {
    let LoweringErrorKind::Coded {
        code,
        message,
        help,
        notes,
    } = &error.kind
    else {
        panic!("expected a coded refusal, got {error:?}");
    };
    assert_eq!(*code, DiagnosticCode::MirPolymorphicRecursion);
    (
        message.clone(),
        help.clone().unwrap_or_default(),
        notes.clone(),
    )
}

/// A class built one `Wrap` deeper on every level is refused at the first
/// instance nesting past the depth bound — `Impl` and 32 `Wrap`s — and the
/// refusal names the chain of instances that led there, in the language's
/// own spelling.
#[test]
fn an_instance_growing_on_every_level_is_refused_past_the_depth_bound() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let main = BodySketch::new()
        .constructs(impl_at(TypeKind::Int))
        .dispatches(op_at(TypeKind::Int), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    let (layers, error) = grow_until_refused(&fixture, &mut demand);
    assert_eq!(layers, MAX_INSTANCE_TYPE_DEPTH);
    let (message, help, notes) = refusal_text(&error);
    assert_eq!(
        message,
        "instantiating `Impl<Wrap<Wrap<…>>>` nests its type argument 33 levels deep"
    );
    assert_eq!(help, DEPTH_THROUGH_TRAIT_HELP);
    assert_eq!(
        notes,
        vec!["each call to `Impl.keep` builds `Impl` at a larger type: \
             Impl<int> → Impl<Wrap<int>> → Impl<Wrap<Wrap<int>>> → …"
            .to_string()]
    );
}

/// The depth bound is a property of the instance alone: one built straight
/// from a root is refused past it, and accepted at it, with no chain behind
/// either.
#[test]
fn the_depth_bound_holds_of_an_instance_built_from_a_root() {
    let fixture = Fixture::new();
    let at_the_bound = BodySketch::new()
        .constructs(impl_at(wrapped_int(MAX_INSTANCE_TYPE_DEPTH - 1)))
        .body();
    fixture.record(&mut VtableDemand::default(), &[("main", at_the_bound)]);

    let past_the_bound = BodySketch::new()
        .constructs(impl_at(wrapped_int(MAX_INSTANCE_TYPE_DEPTH)))
        .body();
    let error = fixture
        .try_record(&mut VtableDemand::default(), &[("main", past_the_bound)])
        .expect_err("an instance past the depth bound is refused");
    let (message, _, notes) = refusal_text(&error);
    assert!(message.contains("33 levels deep"), "{message}");
    assert!(notes.is_empty(), "{notes:?}");
}

/// `Buf<int, size>`, the size written as a value argument.
fn buf_at(size: i128) -> TypeKind {
    TypeKind::Custom(
        "Buf".to_string(),
        Some(vec![
            argument(TypeKind::Int),
            Expression {
                id: 0,
                span: span(),
                node: ExpressionKind::Literal(miri::ast::factory::int_literal(size)),
            },
        ]),
    )
}

/// A root building one class at `sizes`, each its own instantiation.
fn building_bufs(sizes: impl Iterator<Item = i128>) -> Body {
    sizes
        .fold(BodySketch::new(), |sketch, size| {
            sketch.constructs(buf_at(size))
        })
        .body()
}

/// One class may be built at as many value arguments as the bound allows,
/// and no more; the refusal names the parameter the values are for.
#[test]
fn a_class_built_at_more_values_than_the_bound_is_refused() {
    let fixture = Fixture::new();
    let bound = MAX_VALUE_INSTANCES_PER_CLASS as i128;
    let within = building_bufs(1..=bound);
    fixture.record(&mut VtableDemand::default(), &[("main", within)]);

    let past = building_bufs(1..=bound + 1);
    let error = fixture
        .try_record(&mut VtableDemand::default(), &[("main", past)])
        .expect_err("a class past the value bound is refused");
    let (message, help, _) = refusal_text(&error);
    assert_eq!(
        message,
        "instantiating `Buf<int, 257>`: `Buf` needs more than 256 instantiations of `Size`"
    );
    assert_eq!(help, VALUE_HELP);
}

/// Whether a class passes the value bound depends on the set of values it is
/// built at, never on the order they are reached in: the same set, reached
/// largest first and across several bodies, is refused all the same.
#[test]
fn the_value_bound_does_not_depend_on_the_order_values_are_reached() {
    let fixture = Fixture::new();
    let bound = MAX_VALUE_INSTANCES_PER_CLASS as i128;
    let mut demand = VtableDemand::default();
    let first = building_bufs((bound / 2 + 1..=bound + 1).rev());
    fixture.record(&mut demand, &[("main", first)]);
    let again = building_bufs((bound / 2 + 1..=bound + 1).rev());
    let rest = building_bufs((1..=bound / 2).rev());
    let error = fixture
        .try_record(&mut demand, &[("helper", again), ("other", rest)])
        .expect_err("the same set of values is refused in any order");
    let (message, _, _) = refusal_text(&error);
    assert!(message.contains("needs more than 256"), "{message}");
}

/// A struct method spells its explicit `self` as `Self`, which the type table
/// does not define; that is not a type parameter left open, so a drop hook
/// codegen calls from outside any MIR call still counts as reached and fills
/// the slot it calls.
#[test]
fn a_body_spelling_self_is_not_left_open() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let hook = BodySketch::new()
        .receives_self()
        .constructs(impl_at(TypeKind::Int))
        .dispatches(op_at(TypeKind::Int), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("Pt_drop", hook)]);
    assert_eq!(demand.take_named_slot_symbols(), vec!["Impl_keep__int"]);
}

/// The same instance built from a root, where nothing it grows past was
/// reached, is compiled for its own arguments.
#[test]
fn an_instance_built_from_a_root_is_compiled_for_its_own_arguments() {
    let fixture = Fixture::new();
    let keep = fixture.slot("keep");
    let wrapped = generic("Wrap", vec![TypeKind::Int]);
    let main = BodySketch::new()
        .constructs(impl_at(wrapped.clone()))
        .dispatches(op_at(wrapped), keep)
        .body();
    let mut demand = VtableDemand::default();
    fixture.record(&mut demand, &[("main", main)]);
    assert_eq!(
        demand.take_named_slot_symbols(),
        vec!["Impl_keep__Wrap_int"]
    );
}
