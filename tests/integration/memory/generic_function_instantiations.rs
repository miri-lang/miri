// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A generic function is compiled once for each set of type arguments it is
//! called at, so an instantiation it builds from its own parameters is
//! built at whatever its callers bind them to: `Box<T>(x)` inside
//! `shelve_any<T>` called at `Handle` builds a `Box<Handle>`. Such an
//! instantiation counts toward every rule a directly written one does: a
//! trait or a base class above a resource instantiation is a resource, and a
//! method storing a type parameter twice is refused at a resource copy. The
//! instantiations are followed through nested calls, through types built
//! around a parameter, and through the bodies of the methods those calls
//! reach. Every runnable test reads the exact order of the hooks' output
//! under the heap guard.

use super::super::utils::*;

const HANDLE: &str = r#"
class Handle
    public var id int
    fn init(id int)
        self.id = id
    fn drop(self)
        println(f"closed {self.id}")

class Wrap<T>
    public var item T
    fn init(item T)
        self.item = item
"#;

const SHELVING: &str = r#"
struct TraitShelf
    slot Shelving?

trait Shelving
    fn label() int

class Box<T> implements Shelving
    public var item T
    fn init(item T)
        self.item = item
    fn label() int
        return 1

fn keep_shelving(s Shelving)
    if true
        let _shelf = TraitShelf(slot: Some(s))
        println("kept")
    println("leaving keep")
"#;

fn with_shelving(program: &str) -> String {
    format!("{HANDLE}\n{SHELVING}\n{program}")
}

const SHELVE_ANY: &str = r#"
fn shelve_any<T>(x T) Shelving
    return Box<T>(x)
"#;

/// A trait value built inside a generic function called at a resource holds
/// a resource instantiation, so a function keeping it takes it over: the hook
/// runs where the keeper's shelf ends, inside the call.
#[test]
fn a_trait_value_built_in_a_generic_function_at_a_resource_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{SHELVE_ANY}\n{}",
            r#"
fn main()
    let s = shelve_any(Handle(5))
    keep_shelving(s)
    println("after keep")
"#
        )),
        "kept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// The same trait value, once a keeping function took it over, cannot be
/// handed on again.
#[test]
fn a_trait_value_built_in_a_generic_function_at_a_resource_handed_twice_is_reported() {
    let program = with_shelving(&format!(
        "{SHELVE_ANY}\n{}",
        r#"
fn main()
    let s = shelve_any(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":42:19");
}

/// A generic function building its instantiation only at a plain type
/// leaves the trait plain: a keeping function is lent the value, which stays
/// usable.
#[test]
fn a_trait_value_built_in_a_generic_function_at_a_plain_type_stays_usable() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{SHELVE_ANY}\n{}",
            r#"
fn main()
    let s = shelve_any(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "kept\nleaving keep\nkept\nleaving keep\nend",
    );
}

/// An instantiation built in a generic function another generic function
/// calls at its own parameter is built at what the outer call binds.
#[test]
fn a_trait_value_built_two_generic_calls_deep_at_a_resource_handed_twice_is_reported() {
    let program = with_shelving(&format!(
        "{SHELVE_ANY}\n{}",
        r#"
fn shelve_through<T>(x T) Shelving
    return shelve_any(x)

fn main()
    let s = shelve_through(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
}

/// The same nested instantiation, handed to one keeper, is released there.
#[test]
fn a_trait_value_built_two_generic_calls_deep_at_a_resource_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{SHELVE_ANY}\n{}",
            r#"
fn shelve_through<T>(x T) Shelving
    return shelve_any(x)

fn main()
    let s = shelve_through(Handle(5))
    keep_shelving(s)
    println("after keep")
"#
        )),
        "kept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// An instantiation built around a parameter, `Box<Wrap<T>>` at `T` bound
/// to `Handle`, holds the resource through the type it wraps.
#[test]
fn a_trait_value_built_around_a_parameter_at_a_resource_handed_twice_is_reported() {
    let program = with_shelving(
        r#"
fn shelve_wrapped<T>(x T) Shelving
    return Box<Wrap<T>>(Wrap<T>(x))

fn main()
    let s = shelve_wrapped(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// The wrapped instantiation, handed to one keeper, is released there.
#[test]
fn a_trait_value_built_around_a_parameter_at_a_resource_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_shelving(
            r#"
fn shelve_wrapped<T>(x T) Shelving
    return Box<Wrap<T>>(Wrap<T>(x))

fn main()
    let s = shelve_wrapped(Handle(5))
    keep_shelving(s)
    println("after keep")
"#,
        ),
        "kept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// A method a generic function reaches builds its own instantiations at the
/// arguments the function's call binds.
#[test]
fn a_trait_value_built_in_a_method_a_generic_function_reaches_handed_twice_is_reported() {
    let program = with_shelving(
        r#"
class Maker<T>
    fn init()
        println("maker")
    fn make(x T) Shelving
        return Box<T>(x)

fn shelve_made<T>(x T) Shelving
    let m = Maker<T>()
    return m.make(x)

fn main()
    let s = shelve_made(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A method reached only through a trait slot builds its instantiations at
/// the arguments of the class instance converted to the trait.
#[test]
fn a_trait_value_built_in_a_method_reached_through_a_trait_slot_handed_twice_is_reported() {
    let program = with_shelving(
        r#"
trait Factory<T>
    fn make(h T) Shelving

class Maker<T> implements Factory<T>
    fn init()
        println("maker")
    fn make(h T) Shelving
        return Box<T>(h)

fn main()
    let f Factory<Handle> = Maker<Handle>()
    let s = f.make(Handle(5))
    keep_shelving(s)
    keep_shelving(s)
"#,
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

const BASES: &str = r#"
struct BaseShelf
    slot Base?

class Base
    fn init()
        println("base")

class Holding<T> extends Base
    public var item T
    fn init(item T)
        super.init()
        self.item = item

fn hold_any<T>(x T) Base
    return Holding<T>(x)

fn keep_base(b Base)
    if true
        let _shelf = BaseShelf(slot: Some(b))
        println("kept")
    println("leaving keep")
"#;

/// A base class extended by a generic class a generic function builds at a
/// resource is a resource: a keeping function takes the value over, and a
/// second handing is refused.
#[test]
fn a_base_value_built_in_a_generic_function_at_a_resource_handed_twice_is_reported() {
    let program = format!(
        "{HANDLE}\n{BASES}\n{}",
        r#"
fn main()
    let b = hold_any(Handle(5))
    keep_base(b)
    keep_base(b)
"#
    );
    assert_compiler_error(&program, "MER_OWN_003");
}

/// The same base value, handed to one keeper, is released there.
#[test]
fn a_base_value_built_in_a_generic_function_at_a_resource_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &format!(
            "{HANDLE}\n{BASES}\n{}",
            r#"
fn main()
    let b = hold_any(Handle(5))
    keep_base(b)
    println("after keep")
"#
        ),
        "base\nkept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// A generic function that calls itself at a type built around its own
/// parameter would need an instantiation at every depth. Following its
/// instantiations stops at the first one past the depth one instance may
/// nest to, and the program is refused there, rather than compiling forever.
#[test]
fn a_generic_function_recursing_at_a_growing_type_is_refused_and_ends() {
    assert_compiler_error(
        &with_shelving(
            r#"
fn grow<T>(x T, n int) int
    if n == 0
        return 0
    return grow(Box<T>(x), n - 1)

fn main()
    println(f"{grow(Handle(5), 3)}")
"#,
        ),
        "grows on every call",
    );
}

const PAIRS: &str = r#"
class P<A, B>
    public var a A
    public var b B
    fn init(a A, b B)
        self.a = a
        self.b = b

fn pr<A, B>(a A, b B) P<A, B>
    return P<A, B>(a, b)
"#;

/// A balanced tree of `pr` calls `depth` levels deep whose leftmost leaf is a
/// handle and every other leaf an integer.
fn pair_tree(depth: usize, leftmost: bool) -> String {
    match (depth, leftmost) {
        (0, true) => "Handle(5)".to_string(),
        (0, false) => "1".to_string(),
        (_, _) => format!(
            "pr({}, {})",
            pair_tree(depth - 1, leftmost),
            pair_tree(depth - 1, false)
        ),
    }
}

/// A trait value built in a generic function around a type built from 127
/// types, a handle among them, is a resource like one built around the handle
/// alone: the second function keeping it is refused at its use.
#[test]
fn a_trait_value_built_around_a_large_type_holding_a_resource_handed_twice_is_reported() {
    let program = with_shelving(&format!(
        "{SHELVE_ANY}\n{PAIRS}\nfn main()\n    let t = {}\n    let s = shelve_any(t)\n    keep_shelving(s)\n    keep_shelving(s)\n",
        pair_tree(6, true)
    ));
    assert_compiler_error(&program, "MER_OWN_003");
    assert_compiler_error(&program, ":53:19");
}

/// `levels` calls of a generic function wrapping its argument once more,
/// around a handle.
fn wrapped_handle(levels: usize) -> String {
    format!("{}Handle(5){}", "wrap(".repeat(levels), ")".repeat(levels))
}

const WRAP_ANY: &str = r#"
fn wrap<T>(x T) Wrap<T>
    return Wrap<T>(x)
"#;

/// A box around a handle wrapped 31 times nests 32 type constructors deep,
/// the most one instance may: it compiles, and the keeping function takes
/// it over.
#[test]
fn a_trait_value_built_around_a_type_nested_to_the_depth_bound_is_released_by_the_keeper() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{SHELVE_ANY}\n{WRAP_ANY}\nfn main()\n    let s = shelve_any({})\n    keep_shelving(s)\n    println(\"after keep\")\n",
            wrapped_handle(31)
        )),
        "kept\nclosed 5\nleaving keep\nafter keep",
    );
}

/// One wrap more, and the box nests past the depth bound: the program is
/// refused, naming the instance.
#[test]
fn a_trait_value_built_around_a_type_nested_past_the_depth_bound_is_refused() {
    let program = with_shelving(&format!(
        "{SHELVE_ANY}\n{WRAP_ANY}\nfn main()\n    let s = shelve_any({})\n    keep_shelving(s)\n",
        wrapped_handle(32)
    ));
    assert_compiler_error(&program, "MER_MIR_016");
    assert_compiler_error(
        &program,
        "instantiating `Box<Wrap<Wrap<…>>>` nests its type argument 33 levels deep",
    );
}

const MAKER: &str = r#"
trait Maker<T>
    fn make() int

class Impl<T> implements Maker<T>
    public var v T
    fn init(v T)
        self.v = v
    fn make() int
        let b = Box<T>(self.v)
        return b.label()

fn as_maker<T>(v T) Maker<T>
    return Impl<T>(v)
"#;

/// A slot is compiled only for a call through a receiver the instance can
/// stand behind: `make` is called through `Maker<int>`, so the `Impl<Handle>`
/// built in `as_maker` never runs its `make`, and the `Box<Handle>` that
/// `make` would build is never built. The trait above `Box` stays plain, and
/// a keeping function is lent its value, which stays usable.
#[test]
fn a_resource_instance_only_an_unread_slot_would_build_leaves_the_trait_plain() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{MAKER}\n{}",
            r#"
fn main()
    let unused = as_maker(Handle(7))
    let m = as_maker(3)
    println(f"{m.make()}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "1\nkept\nleaving keep\nkept\nleaving keep\nend\nclosed 7",
    );
}

/// The same slot, read through a receiver the `Impl<Handle>` stands behind,
/// builds the `Box<Handle>`, so the trait above it is a resource and its
/// value cannot be handed on twice.
#[test]
fn a_resource_instance_a_read_slot_builds_makes_the_trait_a_resource() {
    let program = with_shelving(&format!(
        "{MAKER}\n{}",
        r#"
fn main()
    let h = as_maker(Handle(7))
    println(f"{h.make()}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A call through a trait written in a generic function nothing calls reads
/// no slot: its receiver `Maker<T>` is never bound to a type, so the
/// `Impl<Handle>` built in `as_maker` never runs its `make`, and the trait
/// above `Box` stays plain.
#[test]
fn a_trait_call_in_a_generic_function_nothing_calls_leaves_the_trait_plain() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{MAKER}\n{}",
            r#"
fn never_called<T>(m Maker<T>) int
    return m.make()

fn main()
    let unused = as_maker(Handle(7))
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "kept\nleaving keep\nkept\nleaving keep\nend\nclosed 7",
    );
}

/// A call through a trait written in a generic function reads the slot only
/// at the types that function is called at: called at `int` alone, its
/// `Maker<T>` is a `Maker<int>`, which the `Impl<Handle>` never stands
/// behind, so the trait above `Box` stays plain.
#[test]
fn a_trait_call_in_a_generic_function_called_at_a_plain_type_leaves_the_trait_plain() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{MAKER}\n{}",
            r#"
fn make_through<T>(m Maker<T>) int
    return m.make()

fn main()
    let unused = as_maker(Handle(7))
    let m = as_maker(3)
    println(f"{make_through(m)}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "1\nkept\nleaving keep\nkept\nleaving keep\nend\nclosed 7",
    );
}

/// A call through a trait written in a closure belongs to the body the
/// closure is written in, so a closure in `main` reads the `Impl<Handle>`
/// slot, builds the `Box<Handle>`, and the trait above it is a resource.
#[test]
fn a_trait_call_in_a_closure_in_main_makes_the_trait_a_resource() {
    let program = with_shelving(&format!(
        "{MAKER}\n{}",
        r#"
fn main()
    let h = as_maker(Handle(7))
    let run = fn() int: h.make()
    println(f"{run()}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
}

/// A call through a field reads the slot at the field's declared trait
/// arguments: `holder.m` is a `Maker<int>`, which the `Impl<Handle>` never
/// stands behind, so the trait above `Box` stays plain.
#[test]
fn a_trait_call_through_a_field_reads_the_slot_at_the_fields_type() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{MAKER}\n{}",
            r#"
struct MakerHolder
    m Maker<int>

fn main()
    let unused = as_maker(Handle(7))
    let holder = MakerHolder(m: as_maker(3))
    println(f"{holder.m.make()}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "1\nkept\nleaving keep\nkept\nleaving keep\nend\nclosed 7",
    );
}

/// An iterable whose `iterator` a subclass overrides, so a `for` loop over it
/// calls `next` through the `Iterator` trait. `Gen<T>`'s `next` builds a
/// `Box<T>`; nothing hands a `Gen` out through `Src` itself.
const WALKER: &str = r#"
class Src<T> implements Iterable<T>
    public var v T
    fn init(v T)
        self.v = v
    public fn iterator() Iterator<T>
        return Plain<T>()

class SubSrc<T> extends Src<T>
    fn init(v T)
        super.init(v)
    public fn iterator() Iterator<T>
        return Gen<T>(self.v)

class Gen<T> implements Iterator<T>
    public var v T
    fn init(v T)
        self.v = v
    public fn next() T?
        let b = Box<T>(self.v)
        if b.label() > 0: return None
        return None

class Plain<T> implements Iterator<T>
    fn init()
        let unused = 0
    public fn next() T?
        return None

fn count_all<T>(s Src<T>) int
    var n = 0
    for _x in s
        n += 1
    return n
"#;

/// A `for` loop in a generic function calls `next` through the `Iterator`
/// trait at the types that function is called at: called at `int` alone, it
/// reads no slot of the `Gen<Handle>` converted to `Iterator<Handle>`, so
/// the `Box<Handle>` that slot would build is never built and the trait
/// above `Box` stays plain.
#[test]
fn a_trait_iterator_loop_in_a_generic_function_called_at_a_plain_type_leaves_the_trait_plain() {
    assert_heap_guard_output(
        &with_shelving(&format!(
            "{WALKER}\n{}",
            r#"
fn main()
    let unused Iterator<Handle> = Gen<Handle>(Handle(7))
    println(f"{count_all(Src<int>(3))}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
    println("end")
"#
        )),
        "0\nkept\nleaving keep\nkept\nleaving keep\nend\nclosed 7",
    );
}

/// The same loop, in the function called at `Handle`, calls `next` through
/// `Iterator<Handle>`, which the `Gen<Handle>` stands behind: its slot builds
/// the `Box<Handle>`, so the trait above it is a resource and its value
/// cannot be handed on twice.
#[test]
fn a_trait_iterator_loop_in_a_generic_function_called_at_a_resource_makes_the_trait_a_resource() {
    let program = with_shelving(&format!(
        "{WALKER}\n{}",
        r#"
fn main()
    let unused Iterator<Handle> = Gen<Handle>(Handle(7))
    println(f"{count_all(Src<Handle>(Handle(8)))}")
    let s Shelving = Box<int>(6)
    keep_shelving(s)
    keep_shelving(s)
"#
    ));
    assert_compiler_error(&program, "MER_OWN_003");
}

/// What checking `program` reports, whatever it says.
fn checked_report(program: &str) -> String {
    crate::utils::miri_check(program).output()
}

/// The help on a type argument growing on every static call.
const STATIC_GROWTH_HELP: &str = "  = help: the type argument grows on every call; bound the recursion by type, or keep one type (e.g. store the depth as a value instead of in the type)\n";

/// A generic function calling itself at a tuple of its own argument doubles
/// that argument's size on every call: the program is refused where the
/// function is declared, as the instantiation past the bound is one of it.
#[test]
fn a_generic_function_doubling_its_argument_is_refused_at_its_declaration() {
    let report = checked_report(
        r#"
use system.io

fn grow<T>(x T, n int) int
    if n == 0: return 0
    return grow((x, x), n - 1)

fn main()
    println(f"{grow(1, 3)}")
"#,
    );
    let expected = format!(
        ":4:1\n   |\n 4 | fn grow<T>(x T, n int) int\n   | ^^^^^^^^^^^^^^^^^^^^^^^^^^ \
         instantiating `grow<((…, …), (…, …))>` builds its type argument from more types \
         than one instance may hold (1024)\n   |\n{STATIC_GROWTH_HELP}   |\n"
    );
    assert!(report.contains("error[MER_MIR_016]"), "{report}");
    assert!(
        report.contains(&expected),
        "expected {expected:?} in:\n{report}"
    );
    assert!(!report.contains("= note:"), "no chain is named:\n{report}");
}

/// The same growth in a generic method is refused where the method is
/// declared.
#[test]
fn a_generic_method_doubling_its_argument_is_refused_at_its_declaration() {
    let report = checked_report(
        r#"
use system.io

class G
    fn grow<T>(x T, n int) int
        if n == 0: return 0
        return self.grow((x, x), n - 1)

fn main()
    let g = G()
    println(f"{g.grow(1, 3)}")
"#,
    );
    let expected = format!(
        ":5:5\n   |\n 5 |     fn grow<T>(x T, n int) int\n   |     ^^^^^^^^^^^^^^^^^^^^^^^^^^ \
         instantiating `grow<((…, …), (…, …))>` builds its type argument from more types \
         than one instance may hold (1024)\n   |\n{STATIC_GROWTH_HELP}   |\n"
    );
    assert!(report.contains("error[MER_MIR_016]"), "{report}");
    assert!(
        report.contains(&expected),
        "expected {expected:?} in:\n{report}"
    );
}

/// A generic function holding a list of its own argument and calling itself
/// with it nests the list one level deeper on every call: the program is
/// refused at the binding declared at the list's type, naming each call's
/// list as a static call grows it.
#[test]
fn a_list_nested_on_every_call_of_a_generic_function_is_refused_with_its_chain() {
    let report = checked_report(
        r#"
use system.collections.list

fn nest<T>(a T, n int) int
    if n == 0
        return 0
    var l = List<T>()
    l.push(a)
    return 1 + nest(l, n - 1)

fn main()
    println(f"{nest(1, 3)}")
"#,
    );
    let expected = format!(
        ":7:5\n   |\n 7 |     var l = List<T>()\n   |     ^^^ instantiating \
         `List<List<List<…>>>` nests its type argument 33 levels deep\n   |\n\
         {STATIC_GROWTH_HELP}   |\n  = note: each call builds `List` at a larger type: \
         List<int> → List<List<int>> → List<List<List<int>>> → …\n"
    );
    assert!(report.contains("error[MER_MIR_016]"), "{report}");
    assert!(
        report.contains(&expected),
        "expected {expected:?} in:\n{report}"
    );
}

/// A struct a generic function builds around its own argument and hands to
/// itself grows on every call: the program is refused where the body builds
/// it, not where the call hands it on.
#[test]
fn a_struct_built_around_the_argument_of_a_generic_function_is_refused_where_it_is_built() {
    let report = checked_report(
        r#"
use system.io

struct S<T>
    v T

fn nest<T>(x T, n int) int
    if n == 0
        return 0
    return 1 + nest(S(v: x), n - 1)

fn main()
    let s = "a" + "b"
    println(f"{nest(s, 3)}")
"#,
    );
    let expected = format!(
        ":10:21\n    |\n 10 |     return 1 + nest(S(v: x), n - 1)\n    |                     \
         ^^^^^^^ instantiating `S<S<S<…>>>` nests its type argument 33 levels deep\n"
    );
    assert!(report.contains("error[MER_MIR_016]"), "{report}");
    assert!(
        report.contains(&expected),
        "expected {expected:?} in:\n{report}"
    );
}
