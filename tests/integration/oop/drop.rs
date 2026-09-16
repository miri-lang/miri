// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use super::utils::*;

#[test]
fn test_class_drop_with_string_field() {
    // String field must not corrupt memory when the class is dropped.
    // String literals are immortal (RC not tracked), so this validates
    // the drop path runs without crashing.
    assert_runs_with_output(
        r#"

class Person
    var name String
    var age int

fn main()
    let p = Person(name: "Alice", age: 30)
    println(p.name)
    println(f"{p.age}")
    "#,
        "Alice\n30",
    );
}

#[test]
fn test_class_drop_with_list_field() {
    // A List field must be DecRef'd (and freed if RC reaches 0) when the class
    // instance is dropped. Without the fix, the List would leak (RC stays 2).
    assert_runs_with_output(
        r#"
use system.collections.list

class Container
    var items [int]
    var count int

fn main()
    let c = Container(items: List([10, 20, 30]), count: 3)
    println(f"{c.count}")
    println(f"{c.items.length()}")
    "#,
        "3\n3",
    );
}

#[test]
fn test_class_drop_with_string_and_list_fields() {
    // Both String and List fields must be handled correctly on drop.
    assert_runs_with_output(
        r#"
use system.collections.list

class Record
    var label String
    var data [int]

fn main()
    let r = Record(label: "test", data: List([1, 2, 3]))
    println(r.label)
    println(f"{r.data.length()}")
    "#,
        "test\n3",
    );
}

#[test]
fn test_class_drop_in_function_scope() {
    // Class with managed fields created inside a helper function. Fields must
    // be released when the local goes out of scope, not just at program exit.
    assert_runs_with_output(
        r#"
use system.collections.list

class Record
    var name String
    var data [int]

fn make_and_count() int
    let r = Record(name: "temp", data: List([1, 2, 3, 4, 5]))
    r.data.length()

fn main()
    let n = make_and_count()
    println(f"{n}")
    "#,
        "5",
    );
}

#[test]
fn test_class_drop_multiple_instances() {
    // Multiple class instances with managed fields all going out of scope.
    assert_runs_with_output(
        r#"
use system.collections.list

class Item
    var name String
    var values [int]

fn main()
    let a = Item(name: "first", values: List([1, 2]))
    let b = Item(name: "second", values: List([3, 4, 5]))
    println(a.name)
    println(f"{b.values.length()}")
    "#,
        "first\n3",
    );
}

#[test]
fn test_class_list_field_reassign() {
    // Reassigning a managed field variable frees the old value.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List([1, 2, 3])
    l = List([4, 5])
    println(f"{l.length()}")
    "#,
        "2",
    );
}

// ── Nested / complex drop scenarios ──────────────────────────────────────────

#[test]
fn test_class_with_nested_class_field_drops_correctly() {
    // Inner class is heap-allocated and reference-counted. Dropping Outer must
    // DecRef Inner; if Inner's RC reaches zero, Inner is freed too.
    assert_runs_with_output(
        r#"

class Inner
    var x int

class Outer
    var child Inner

fn make() int
    let o = Outer(child: Inner(x: 99))
    o.child.x

fn main()
    let v = make()
    println(f"{v}")
    "#,
        "99",
    );
}

#[test]
fn test_reassign_class_field_drops_old_value() {
    // Reassigning a class-typed field must DecRef the old object.
    assert_runs_with_output(
        r#"

class Node
    var value int

class Holder
    var node Node

fn main()
    var h = Holder(node: Node(value: 1))
    h.node = Node(value: 2)
    println(f"{h.node.value}")
    "#,
        "2",
    );
}

#[test]
fn test_object_shared_between_two_variables_not_freed_early() {
    // Assigning the same object to two variables bumps its RC to 2.
    // Neither variable alone should free it.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List([1, 2, 3])
    let l2 = l
    println(f"{l2.length()}")
    "#,
        "3",
    );
}

#[test]
fn test_drop_in_loop() {
    // Object created inside a loop body must be dropped at end of each iteration,
    // not accumulated until the loop exits.
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var i = 0
    while i < 3
        let tmp = List([i])
        i += 1
    println("ok")
    "#,
        "ok",
    );
}

// ── User-defined drop hook ──────────────────────────────────────────────────────

#[test]
fn test_user_drop_hook_called_at_scope_exit() {
    assert_runs_with_output(
        r#"

struct Res
    handle int
    fn drop(self)
        println("dropped")

fn main()
    let r = Res(handle: 42)
"#,
        "dropped",
    );
}

#[test]
fn test_user_drop_hook_called_before_parent_returns() {
    assert_runs_with_output(
        r#"

struct Token
    id int
    fn drop(self)
        println("token gone")

fn use_token()
    let t = Token(id: 1)
    println("using")

fn main()
    use_token()
    println("after")
"#,
        "using\ntoken gone\nafter",
    );
}

#[test]
fn test_user_drop_hook_multiple_fields_access() {
    // Drop hook can use self fields (via self.x pattern if supported),
    // but here we just verify the hook is called even when struct has multiple fields.
    assert_runs_with_output(
        r#"

struct Handle
    fd int
    flags int
    fn drop(self)
        println("handle closed")

fn main()
    let h = Handle(fd: 3, flags: 0)
    println("opened")
"#,
        "opened\nhandle closed",
    );
}

// ── Scope-exit warning for unconsumed resources ─────────────────────────────────

#[test]
fn test_scope_exit_warning_emitted_for_unconsumed_resource() {
    assert_compiler_warning(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn main()
    let conn = Conn(handle: 1)
    println("working")
"#,
        "resource 'conn' of type 'Conn' was not consumed before scope exit",
    );
}

#[test]
fn test_scope_exit_warning_suppressed_when_resource_consumed() {
    // Passing to a consuming function suppresses the warning.
    assert_type_checks(
        r#"

struct Conn
    handle int
    fn drop(self)
        return

fn sink(c Conn)
    return

fn main()
    let conn = Conn(handle: 1)
    sink(conn)
"#,
    );
}

#[test]
fn test_scope_exit_warning_in_nested_scope() {
    // Resource declared inside a helper function warns at function exit.
    assert_compiler_warning(
        r#"

struct Token
    id int
    fn drop(self)
        return

fn use_token()
    let t = Token(id: 1)
    println("using")

fn main()
    use_token()
"#,
        "resource 't' of type 'Token' was not consumed before scope exit",
    );
}

#[test]
fn test_scope_exit_no_warning_for_non_resource_struct() {
    // Structs without fn drop are not resource types — no warning.
    assert_type_checks(
        r#"

struct Point
    x int
    y int

fn main()
    let p = Point(x: 1, y: 2)
    println(f"{p.x}")
"#,
    );
}

/// Runs `code` and requires its standard output to be exactly `expected`, so a
/// hook that fires twice, or not at all, cannot pass on a substring match.
fn assert_stdout_is(code: &str, expected: &str) {
    let result = crate::utils::miri_run(code);
    assert!(
        result.success,
        "expected the program to run: {}",
        result.output()
    );
    assert_eq!(result.stdout.trim(), expected, "{}", result.output());
}

#[test]
fn test_class_drop_hook_runs_once_at_scope_exit() {
    assert_stdout_is(
        r#"
class Handle
    public var id int

    public fn init(id int)
        self.id = id

    public fn drop(self)
        println("dropped")

fn main()
    var h = Handle(1)
    println(f"{h.id}")
"#,
        "1\ndropped",
    );
}

#[test]
fn test_class_drop_hook_reads_the_instance_it_releases() {
    assert_stdout_is(
        r#"
class Handle
    public var id int

    public fn drop(self)
        println(f"closing {self.id}")

fn open(id int)
    let h = Handle(id: id)
    println(f"opened {h.id}")

fn main()
    open(7)
    open(8)
    println("done")
"#,
        "opened 7\nclosing 7\nopened 8\nclosing 8\ndone",
    );
}

#[test]
fn test_class_field_holding_a_resource_runs_its_drop_hook() {
    assert_stdout_is(
        r#"
class Handle
    public var id int

    public fn drop(self)
        println(f"handle {self.id} dropped")

class Owner
    public var handle Handle

fn main()
    var owner = Owner(handle: Handle(id: 3))
    println(f"owns {owner.handle.id}")
"#,
        "owns 3\nhandle 3 dropped",
    );
}

#[test]
fn test_class_inherits_drop_hook_from_its_base() {
    assert_stdout_is(
        r#"
class Base
    public var id int

    public fn drop(self)
        println(f"base drop {self.id}")

class Child extends Base
    public var extra int

fn main()
    var c = Child(id: 4, extra: 5)
    println(f"child {c.extra}")
"#,
        "child 5\nbase drop 4",
    );
}

#[test]
fn test_class_declared_before_its_base_inherits_drop_hook() {
    assert_stdout_is(
        r#"
class Child extends Base
    public var extra int

class Base
    public var id int

    public fn drop(self)
        println("base drop")

fn main()
    var c = Child(id: 1, extra: 2)
    println(f"child {c.extra}")
"#,
        "child 2\nbase drop",
    );
}

#[test]
fn test_class_inherits_drop_hook_from_abstract_base() {
    assert_stdout_is(
        r#"
abstract class Resource
    public var id int

    abstract fn label() String

    public fn drop(self)
        println(f"release {self.label()}")

class File extends Resource
    public fn label() String
        return "file"

fn main()
    var f = File(id: 1)
    println(f"id {f.id}")
"#,
        "id 1\nrelease file",
    );
}

#[test]
fn test_overriding_drop_hook_runs_only_the_override() {
    assert_stdout_is(
        r#"
class Base
    public var id int

    public fn drop(self)
        println("base drop")

class Child extends Base
    public fn drop(self)
        println("child drop")

fn main()
    var c = Child(id: 1)
    println(f"id {c.id}")
"#,
        "id 1\nchild drop",
    );
}

#[test]
fn test_class_with_drop_hook_is_a_resource() {
    assert_compiler_warning(
        r#"
class Conn
    public var handle int

    public fn drop(self)
        return

fn main()
    let conn = Conn(handle: 1)
    println("working")
"#,
        "resource 'conn' of type 'Conn' was not consumed before scope exit",
    );
}

#[test]
fn test_class_drop_hook_runs_for_each_list_element() {
    assert_stdout_is(
        r#"
use system.collections.list

class Handle
    public var id int

    public fn drop(self)
        println(f"drop {self.id}")

fn main()
    var handles = List<Handle>()
    handles.push(Handle(id: 1))
    handles.push(Handle(id: 2))
    println(f"{handles.length()}")
"#,
        "2\ndrop 1\ndrop 2",
    );
}

#[test]
fn test_generic_class_drop_hook_runs_for_a_managed_instantiation() {
    assert_stdout_is(
        r#"
class Box<T>
    public var value T

    public fn drop(self)
        println("box dropped")

fn main()
    var b = Box<String>(value: "held")
    println(b.value)
"#,
        "held\nbox dropped",
    );
}

#[test]
fn test_class_drop_hook_spelled_without_self_runs_once() {
    assert_stdout_is(
        r#"
class Handle
    public var id int

    public fn drop()
        println("dropped")

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        "1\ndropped",
    );
}

#[test]
fn test_class_drop_hook_spelled_without_self_is_a_resource() {
    assert_compiler_warning(
        r#"
class Conn
    public var handle int

    public fn drop()
        return

fn main()
    let conn = Conn(handle: 1)
    println("working")
"#,
        "resource 'conn' of type 'Conn' was not consumed before scope exit",
    );
}

#[test]
fn test_calling_drop_hook_spelled_without_self_consumes_the_value() {
    assert_compiler_error(
        r#"
class Handle
    public var id int

    public fn drop()
        println(f"dropped {self.id}")

fn main()
    var h = Handle(id: 2)
    h.drop()
    println(f"{h.id}")
"#,
        "'h' was consumed by 'drop'",
    );
}

#[test]
fn test_trait_default_drop_hook_runs_once() {
    assert_stdout_is(
        r#"
trait Closable
    fn drop(self)
        println("trait drop")

class Handle implements Closable
    public var id int

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        "1\ntrait drop",
    );
}

#[test]
fn test_trait_default_drop_hook_spelled_without_self_runs_once() {
    assert_stdout_is(
        r#"
trait Closable
    fn drop()
        println("trait drop")

class Handle implements Closable
    public var id int

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        "1\ntrait drop",
    );
}

#[test]
fn test_trait_default_drop_hook_makes_the_class_a_resource() {
    assert_compiler_warning(
        r#"
trait Closable
    fn drop(self)
        return

class Conn implements Closable
    public var handle int

fn main()
    let conn = Conn(handle: 1)
    println("working")
"#,
        "resource 'conn' of type 'Conn' was not consumed before scope exit",
    );
}

#[test]
fn test_parent_trait_default_drop_hook_runs_once() {
    assert_stdout_is(
        r#"
trait Closable
    fn drop(self)
        println("closed")

trait Stream extends Closable
    fn name() String

class Pipe implements Stream
    public var id int

    public fn name() String
        return "pipe"

fn main()
    var p = Pipe(id: 1)
    println(p.name())
"#,
        "pipe\nclosed",
    );
}

#[test]
fn test_class_drop_hook_overrides_trait_default() {
    assert_stdout_is(
        r#"
trait Closable
    fn drop(self)
        println("trait drop")

class Handle implements Closable
    public var id int

    public fn drop(self)
        println("class drop")

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        "1\nclass drop",
    );
}

#[test]
fn test_subclass_runs_trait_default_drop_hook_inherited_from_base() {
    assert_stdout_is(
        r#"
trait Closable
    fn drop(self)
        println(f"trait drop {self.id()}")

    fn id() int

class Base implements Closable
    public var key int

    public fn id() int
        return self.key

class Child extends Base
    public var extra int

fn main()
    var c = Child(key: 6, extra: 7)
    println(f"{c.extra}")
"#,
        "7\ntrait drop 6",
    );
}

const DROP_HOOK_SHAPE: &str = "is the drop hook, which takes no arguments";

#[test]
fn test_class_drop_with_parameters_is_rejected() {
    assert_compiler_error(
        r#"
class Handle
    public var id int

    public fn drop(code int)
        println(f"{code}")

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        DROP_HOOK_SHAPE,
    );
}

#[test]
fn test_static_class_drop_is_rejected() {
    assert_compiler_error(
        r#"
class Handle
    public var id int

    public static fn drop()
        println("static")

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        DROP_HOOK_SHAPE,
    );
}

#[test]
fn test_trait_drop_with_parameters_is_rejected() {
    assert_compiler_error(
        r#"
trait Closable
    fn drop(self, code int)
        println(f"{code}")

class Handle implements Closable
    public var id int

fn main()
    var h = Handle(id: 1)
    println(f"{h.id}")
"#,
        DROP_HOOK_SHAPE,
    );
}

#[test]
fn test_struct_drop_without_self_names_the_hook_spelling() {
    assert_compiler_error(
        r#"
struct Point
    x int

    fn drop()
        println("struct drop")

fn main()
    let p = Point(x: 1)
    println(f"{p.x}")
"#,
        "Struct 'Point' declares its drop hook as 'fn drop(self)'",
    );
}

#[test]
fn test_calling_trait_default_drop_hook_runs_it_once_at_the_call() {
    assert_heap_guard_output(
        r#"
trait Closable
    fn drop(self)
        println("trait drop")

class Handle implements Closable
    public var id int

fn main()
    var g = Handle(id: 2)
    g.drop()
    println("end")
"#,
        "trait drop\nend",
    );
}
