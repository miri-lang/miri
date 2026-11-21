// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Every argument to an `out` parameter that names no place a call may take
//! is refused with the binding and the path it names, the rule in one
//! sentence, and a repair. Each refusal is pinned word for word, and beside
//! it the program its help describes runs: a repair that does not compile is
//! no repair.

use super::super::utils::*;
use crate::utils::miri_check_json;

const CELL: &str = r#"
use system.io
use system.collections.list

class Cell
    var n int

fn step(c out Cell)
    c = Cell(n: c.n + 10)
"#;

const SWAP: &str = r#"
use system.io
use system.collections.list

fn swap(a out int, b out int)
    let t = a
    a = b
    b = t
"#;

fn with_cell(program: &str) -> String {
    format!("{CELL}\n{program}")
}

fn with_swap(program: &str) -> String {
    format!("{SWAP}\n{program}")
}

/// Asserts `source` is refused under `code` with exactly `message` and
/// `help`, and returns the diagnostic for the caller to read its repair.
fn assert_refused_exactly(
    source: &str,
    code: &str,
    message: &str,
    help: &str,
) -> miri::diagnostics::json::JsonDiagnostic {
    let envelope = miri_check_json(source);
    let refuses_an_out_argument = |message: &str| {
        message.starts_with("'out' parameter") || message.starts_with("same variable")
    };
    let diagnostic = (envelope.diagnostics.into_iter())
        .find(|d| d.code.as_deref() == Some(code) && refuses_an_out_argument(&d.message))
        .unwrap_or_else(|| panic!("a {code} refusal of an 'out' argument in:\n{source}"));
    assert_eq!(diagnostic.message, message, "{diagnostic:#?}");
    assert_eq!(diagnostic.help.as_deref(), Some(help), "{diagnostic:#?}");
    diagnostic
}

#[test]
fn an_element_of_a_let_root_names_the_var_repair() {
    let diagnostic = assert_refused_exactly(
        &with_cell(
            r#"
fn main()
    let grid = List([Cell(n: 1)])
    step(grid[0])
"#,
        ),
        "MER_TYP_042",
        "'out' parameter 'c' is handed 'grid[0]', but 'grid' is immutable: it is declared with 'let', and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "declare 'grid' with 'var' ('var grid = …'), then pass 'grid[0]'",
    );
    let repair = diagnostic.repair.as_ref().map(|repair| repair.id.as_str());
    assert_eq!(repair, Some("let-to-var"), "{diagnostic:#?}");
}

#[test]
fn an_element_of_a_root_declared_with_var_is_written() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn main()
    var grid = List([Cell(n: 1)])
    step(grid[0])
    println(f"{grid[0].n}")
"#,
        ),
        "11",
    );
}

#[test]
fn an_element_of_a_lent_parameter_names_the_out_parameter_repair() {
    assert_refused_exactly(
        &with_cell(
            r#"
fn step_first(grid List<Cell>)
    step(grid[0])

fn main()
    var grid = List([Cell(n: 1)])
    step_first(grid)
"#,
        ),
        "MER_TYP_042",
        "'out' parameter 'c' is handed 'grid[0]', but 'grid' is immutable: it is a parameter its caller lends, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "declare the parameter 'out' ('grid out List<Cell>') so the call writes its caller's value, or copy it into a 'var' ('var copy = grid'), pass 'copy[0]' and return the copy",
    );
}

#[test]
fn an_element_of_a_parameter_declared_out_is_written_through_to_the_caller() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn step_first(grid out List<Cell>)
    step(grid[0])

fn main()
    var grid = List([Cell(n: 1)])
    step_first(grid)
    println(f"{grid[0].n}")
"#,
        ),
        "11",
    );
}

#[test]
fn an_element_of_a_copy_of_a_lent_parameter_is_written_and_returned() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn stepped_first(grid List<Cell>) List<Cell>
    var copy = grid
    step(copy[0])
    return copy

fn main()
    let grid = List([Cell(n: 1)])
    let stepped = stepped_first(grid)
    println(f"{grid[0].n} {stepped[0].n}")
"#,
        ),
        "1 11",
    );
}

#[test]
fn an_element_of_a_call_result_names_the_binding_repair() {
    assert_refused_exactly(
        &with_cell(
            r#"
fn make() List<Cell>
    return List([Cell(n: 1)])

fn main()
    step(make()[0])
"#,
        ),
        "MER_TYP_054",
        "'out' parameter 'c' is handed 'make()[0]', a place in the result of a call, which no variable holds: a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "bind the result to a 'var' first ('var value = make()'), then pass 'value[0]'",
    );
}

#[test]
fn an_element_of_a_call_result_bound_to_a_var_is_written() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn make() List<Cell>
    return List([Cell(n: 1)])

fn main()
    var value = make()
    step(value[0])
    println(f"{value[0].n}")
"#,
        ),
        "11",
    );
}

#[test]
fn a_literal_names_the_var_holding_it() {
    assert_refused_exactly(
        r#"
use system.io

fn inc(x out int)
    x = x + 1

fn main()
    inc(5)
"#,
        "MER_TYP_054",
        "'out' parameter 'x' is handed a value, which no variable holds: a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "hold the value in a 'var' ('var value = …'), pass 'value', and read the new value from it after the call",
    );
}

#[test]
fn a_value_held_in_a_var_is_written() {
    assert_runs_with_output(
        r#"
use system.io

fn inc(x out int)
    x = x + 1

fn main()
    var value = 5
    inc(value)
    println(f"{value}")
"#,
        "6",
    );
}

#[test]
fn an_entry_of_a_map_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_cell(
            r#"
use system.collections.map

fn main()
    var cells = {"a": Cell(n: 1)}
    step(cells["a"])
"#,
        ),
        "MER_TYP_054",
        "'out' parameter 'c' is handed 'cells[\"a\"]', an entry of the map 'cells', which is not a place: a map reaches an entry only through its key, so a call cannot write the entry in place",
        "copy the entry into a 'var', pass that and store it back under its key: 'var value = cells[\"a\"]', pass 'value', then 'cells[\"a\"] = value'",
    );
}

#[test]
fn an_entry_copied_in_and_stored_back_is_written() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn main()
    var cells = {"a": Cell(n: 1)}
    var value = cells["a"]
    step(value)
    cells["a"] = value
    println(f"{cells['a'].n}")
"#,
        ),
        "11",
    );
}

#[test]
fn a_field_of_an_entry_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        r#"
use system.collections.map

class Cell
    var n int

fn inc(x out int)
    x = x + 1

fn main()
    var cells = {"a": Cell(n: 1)}
    inc(cells["a"].n)
"#,
        "MER_TYP_054",
        "'out' parameter 'x' is handed 'cells[\"a\"].n', a place in an entry of the map 'cells', which is not a place: a map reaches an entry only through its key, so a call cannot write the entry in place",
        "copy the entry into a 'var', pass that and store it back under its key: 'var value = cells[\"a\"]', pass 'value.n', then 'cells[\"a\"] = value'",
    );
}

#[test]
fn an_element_handed_to_a_runtime_function_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        r#"
use system.collections.list

@element("place, new")
@writes("place")
@stores("new")
@returns_element
runtime "core" fn miri_rt_replace<T>(place out T, new T) T

fn main()
    var v = List([1, 2])
    let old = miri_rt_replace(v[0], 5)
"#,
        "MER_TYP_054",
        "'out' parameter 'place' of a runtime function takes a whole variable, not 'v[0]': the runtime writes the variable it is handed and reaches no place inside one",
        "copy it into a 'var', pass that and store it back: 'var value = v[0]', pass 'value', then 'v[0] = value'",
    );
}

#[test]
fn an_element_copied_in_for_a_runtime_function_and_stored_back_is_written() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.list

@element("place, new")
@writes("place")
@stores("new")
@returns_element
runtime "core" fn miri_rt_replace<T>(place out T, new T) T

fn main()
    var v = List([1, 2])
    var value = v[0]
    let old = miri_rt_replace(value, 5)
    v[0] = value
    println(f"{old} {v[0]} {v[1]}")
"#,
        "1 5 2",
    );
}

const DEVICE_NESTED: &str = r#"
use system.io
use system.gpu
use system.collections.array

fn bump(n out int) int
    n = n + 1
    return n

gpu fn fill(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    var grid = [[1, 2], [3, 4]]
    let _ = bump(grid[0][1])
    dst[i] = grid[0][1]

fn main()
    gpu var out = Array<int, 4>()
    fill(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
"#;

#[test]
fn a_nested_element_in_device_code_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        DEVICE_NESTED,
        "MER_TYP_054",
        "'out' parameter 'n' is handed 'grid[0][1]' in device code, which takes only a whole local, an element of a buffer ('dst[i]') or a component of a vector ('v.x', 'pts[i].x')",
        "copy it into a 'var', pass that and store it back: 'var value = grid[0][1]', pass 'value', then 'grid[0][1] = value'",
    );
}

#[test]
fn an_element_in_device_code_stored_from_a_returned_value_is_written() {
    super::super::gpu::device::assert_gpu_runs_with_output(
        r#"
use system.io
use system.gpu
use system.collections.array

fn bumped(n int) int
    return n + 1

gpu fn fill(dst out Array<int, 4>)
    let i = kernel.global_idx.x
    dst[i] = bumped(dst[i])

fn main()
    gpu var out = Array<int, 4>()
    fill(out).launch(Dim3(4, 1, 1), Dim3(1, 1, 1))
    let host = out
    println(f"{host[0]} {host[3]}")
"#,
        "1 1",
    );
}

#[test]
fn an_element_of_a_gpu_var_names_the_host_copy_repair() {
    assert_refused_exactly(
        r#"
use system.io

fn bump(n out int)
    n = n + 1

fn main()
    gpu var g = [1, 2, 3]
    bump(g[0])
"#,
        "MER_TYP_054",
        "'out' parameter 'n' is handed 'g[0]', a place in the gpu-resident 'g', which a call on the host cannot take",
        "copy 'g' to the host ('var host = g'), pass 'host[0]', then copy it back ('g = host')",
    );
}

#[test]
fn an_element_of_a_host_copy_of_a_gpu_var_is_written_and_copied_back() {
    assert_runs_with_output(
        r#"
use system.io

fn bump(n out int)
    n = n + 1

fn main()
    gpu var g = [1, 2, 3]
    var host = g
    bump(host[0])
    g = host
    let seen = g
    println(f"{seen[0]} {seen[1]}")
"#,
        "2 2",
    );
}

#[test]
fn two_elements_of_one_list_name_both_places_and_the_copy_repair() {
    assert_refused_exactly(
        &with_swap(
            r#"
fn main()
    var v = List([1, 2])
    swap(v[0], v[1])
"#,
        ),
        "MER_TYP_066",
        "same variable passed twice as 'out': 'v[0]' and 'v[1]' are both places in 'v', and a call takes a variable once",
        "a call takes each variable once, whichever places in it the arguments name; copy 'v[1]' into a 'var' ('var value = v[1]'), pass 'value', then store it back ('v[1] = value')",
    );
}

/// Two distinct fields of one variable are refused as two elements are: a
/// call takes each variable once, and the help states that rule rather
/// than a doubt about which place is which.
#[test]
fn two_distinct_fields_of_one_variable_state_the_once_per_variable_rule() {
    assert_refused_exactly(
        r#"
use system.collections.list

struct Holder
    tags List<String>
    small List<String>

fn fill(a out List<String>, b out List<String>)
    a = List(["a"])
    b = List(["b"])

fn main()
    var h = Holder(tags: List<String>(), small: List<String>())
    fill(h.tags, h.small)
"#,
        "MER_TYP_066",
        "same variable passed twice as 'out': 'h.tags' and 'h.small' are both places in 'h', and a call takes a variable once",
        "a call takes each variable once, whichever places in it the arguments name; copy 'h.small' into a 'var' ('var value = h.small'), pass 'value', then store it back ('h.small = value')",
    );
}

#[test]
fn one_element_copied_out_for_a_swap_is_stored_back() {
    assert_runs_with_output(
        &with_swap(
            r#"
fn main()
    var v = List([1, 2])
    var value = v[1]
    swap(v[0], value)
    v[1] = value
    println(f"{v[0]} {v[1]}")
"#,
        ),
        "2 1",
    );
}

#[test]
fn one_variable_passed_twice_names_a_second_variable() {
    assert_refused_exactly(
        &with_swap(
            r#"
fn main()
    var x = 1
    swap(x, x)
"#,
        ),
        "MER_TYP_066",
        "same variable passed twice as 'out': 'x' appears more than once",
        "pass a different 'var' to each 'out' parameter",
    );
}

#[test]
fn an_element_of_a_loop_binding_names_the_index_repair() {
    assert_refused_exactly(
        &with_cell(
            r#"
fn main()
    var rows = List([List([Cell(n: 1)])])
    for row in rows
        step(row[0])
"#,
        ),
        "MER_TYP_042",
        "'out' parameter 'c' is handed 'row[0]', but 'row' is immutable: it is the binding of a 'for' loop, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "iterate by index and pass the place through the collection: 'for i in 0..xs.length()', then pass 'xs[i][0]'",
    );
}

#[test]
fn an_element_reached_by_index_in_a_loop_is_written() {
    assert_heap_guard_output(
        &with_cell(
            r#"
fn main()
    var rows = List([List([Cell(n: 1)]), List([Cell(n: 2)])])
    for i in 0..rows.length()
        step(rows[i][0])
    println(f"{rows[0][0].n} {rows[1][0].n}")
"#,
        ),
        "11 12",
    );
}

#[test]
fn a_constant_names_the_var_copy_repair() {
    assert_refused_exactly(
        r#"
fn inc(x out int)
    x = x + 1

const START = 5

fn main()
    inc(START)
"#,
        "MER_TYP_042",
        "'out' parameter 'x' is handed 'START', but 'START' is immutable: it is a constant, and a call writes only a place rooted at a 'var', an 'out' parameter or 'self'",
        "copy it into a 'var' ('var value = START') and pass 'value'",
    );
}

#[test]
fn a_constant_copied_into_a_var_is_written() {
    assert_runs_with_output(
        r#"
use system.io

fn inc(x out int)
    x = x + 1

const START = 5

fn main()
    var value = START
    inc(value)
    println(f"{START} {value}")
"#,
        "5 6",
    );
}

const FROZEN: &str = r#"
use system.io

class Frozen
    var count int
    let fixed int

fn inc(x out int)
    x = x + 1
"#;

#[test]
fn a_field_declared_with_let_names_the_field() {
    assert_refused_exactly(
        &format!(
            "{FROZEN}{}",
            r#"
fn main()
    var f = Frozen(count: 1, fixed: 2)
    inc(f.fixed)
"#
        ),
        "MER_TYP_042",
        "'out' parameter 'x' is handed 'f.fixed', a path through the field 'fixed', which its class declares with 'let'",
        "declare the field 'fixed' with 'var' in its class, then pass 'f.fixed'",
    );
}

#[test]
fn a_field_declared_with_var_is_written() {
    assert_runs_with_output(
        &format!(
            "{FROZEN}{}",
            r#"
fn main()
    var f = Frozen(count: 1, fixed: 2)
    inc(f.count)
    println(f"{f.count} {f.fixed}")
"#
        ),
        "2 2",
    );
}

const HOLDERS: &str = r#"
use system.io
use system.collections.list

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")

class Holder
    var conn Conn

    fn swap_from(c out Conn)
        println(f"self.conn before {self.conn.id}")
        c = Conn(id: 5)
        println(f"self.conn after {self.conn.id}")

class Tagged
    var name String
    var tags List<String>

fn put(c out Conn, h Holder)
    c = Conn(id: 50)
    println(f"h sees {h.conn.id}")

fn put_in_list(c out Conn, v List<Holder>)
    c = Conn(id: 50)
    println(f"v sees {v[0].conn.id}")

fn retag(c out List<String>, h Tagged)
    c = List(["new"])
    println(f"h sees {h.tags[0]} {h.name}")

fn rename(c out List<String>, name String)
    c = List([name])

fn bump(x out int, all List<int>)
    x = x + all.length()
"#;

fn with_holders(program: &str) -> String {
    format!("{HOLDERS}\n{program}")
}

#[test]
fn a_field_beside_the_class_instance_holding_it_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn main()
    var h = Tagged(name: "x", tags: List(["old"]))
    retag(h.tags, h)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'c' is handed 'h.tags', and the call also lends 'h', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = h.tags', pass 'value', then 'h.tags = value'",
    );
}

#[test]
fn a_field_copied_in_beside_the_class_instance_holding_it_is_stored_back() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var h = Tagged(name: "x", tags: List(["old"]))
    var value = h.tags
    retag(value, h)
    h.tags = value
    println(f"{h.tags[0]}")
"#,
        ),
        "h sees old x\nnew",
    );
}

#[test]
fn a_resource_field_beside_the_resource_holding_it_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn main()
    var h = Holder(conn: Conn(id: 1))
    put(h.conn, h)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'c' is handed 'h.conn', and the call also lends 'h', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = h.conn', pass 'value', then 'h.conn = value'",
    );
}

#[test]
fn a_resource_field_copied_in_beside_the_resource_holding_it_is_stored_back() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var h = Holder(conn: Conn(id: 1))
    var value = h.conn
    put(value, h)
    h.conn = value
    println(f"{h.conn.id}")
"#,
        ),
        "h sees 1\nclosed 1\n50\nclosed 50",
    );
}

#[test]
fn a_field_of_the_receiver_handed_to_its_own_method_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn main()
    var h = Holder(conn: Conn(id: 1))
    h.swap_from(h.conn)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'c' is handed 'h.conn', and the call is made on 'h', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = h.conn', pass 'value', then 'h.conn = value'",
    );
}

#[test]
fn a_field_of_the_receiver_copied_in_for_its_own_method_is_stored_back() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var h = Holder(conn: Conn(id: 1))
    var value = h.conn
    h.swap_from(value)
    h.conn = value
    println(f"h.conn {h.conn.id}")
"#,
        ),
        "self.conn before 1\nself.conn after 1\nclosed 1\nh.conn 5\nclosed 5",
    );
}

#[test]
fn a_field_of_an_element_beside_its_list_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn main()
    var v = List([Holder(conn: Conn(id: 1))])
    put_in_list(v[0].conn, v)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'c' is handed 'v[0].conn', and the call also lends 'v', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = v[0].conn', pass 'value', then 'v[0].conn = value'",
    );
}

#[test]
fn a_field_of_an_element_beside_another_element_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn peek(c out Conn, h Holder)
    c = Conn(id: 50)

fn main()
    var v = List([Holder(conn: Conn(id: 1)), Holder(conn: Conn(id: 2))])
    let i = 0
    let j = 1
    peek(v[i].conn, v[j])
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'c' is handed 'v[i].conn', and the call also lends 'v[j]', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = v[i].conn', pass 'value', then 'v[i].conn = value'",
    );
}

#[test]
fn a_field_of_an_element_copied_in_beside_its_list_is_stored_back() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var v = List([Holder(conn: Conn(id: 1))])
    var value = v[0].conn
    put_in_list(value, v)
    v[0].conn = value
    println(f"{v[0].conn.id}")
"#,
        ),
        "v sees 1\nclosed 1\n50\nclosed 50",
    );
}

#[test]
fn an_element_beside_its_list_names_the_copy_in_copy_out_repair() {
    assert_refused_exactly(
        &with_holders(
            r#"
fn main()
    var v = List([1, 2])
    let i = 1
    bump(v[i], v)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'x' is handed 'v[i]', and the call also lends 'v', a value holding that place: a call may not lend a value containing a place it writes",
        "copy it into a 'var', pass that and store it back: 'var value = v[i]', pass 'value', then 'v[i] = value'",
    );
}

#[test]
fn an_element_copied_in_beside_its_list_is_stored_back() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var v = List([1, 2])
    let i = 1
    var value = v[i]
    bump(value, v)
    v[i] = value
    println(f"{v[0]} {v[1]}")
"#,
        ),
        "1 4",
    );
}

#[test]
fn a_field_beside_a_sibling_field_of_one_instance_is_written() {
    assert_heap_guard_output(
        &with_holders(
            r#"
fn main()
    var h = Tagged(name: "x", tags: List(["old"]))
    rename(h.tags, h.name)
    println(f"{h.tags[0]} {h.name}")
"#,
        ),
        "x x",
    );
}

#[test]
fn a_slot_of_a_var_tuple_names_the_rebuilt_tuple_repair() {
    assert_refused_exactly(
        r#"
fn bump(n out int)
    n = n + 1

fn main()
    var t = (1, 2)
    bump(t.0)
"#,
        "MER_TYP_054",
        "'out' parameter 'n' is handed 't.0', a slot of the tuple 't', which is not a place: a tuple's elements are fixed when it is built",
        "copy it into a 'var' and pass that: 'var value = t.0', pass 'value', then build 't' again with 'value' in place of 't.0'",
    );
}

#[test]
fn a_slot_of_a_tuple_copied_in_and_built_again_is_written() {
    assert_runs_with_output(
        r#"
use system.io

fn bump(n out int)
    n = n + 1

fn main()
    var t = (1, 2)
    var value = t.0
    bump(value)
    t = (value, t.1)
    println(f"{t.0} {t.1}")
"#,
        "2 2",
    );
}

const STORE: &str = r#"
use system.io
use system.collections.list

class Conn
    var id int
    fn drop()
        println(f"closed {self.id}")

struct Pair
    c Conn

fn store(r out Conn, keep Conn = r) List<Conn>
    r = Conn(id: 60)
    List([keep])
"#;

/// Asserts `program`, after [`STORE`], is refused under MER_OWN_003 with
/// exactly `message`.
fn assert_default_kept_twice(program: &str, message: &str) {
    let source = format!("{STORE}\n{program}");
    let envelope = miri_check_json(&source);
    let diagnostic = (envelope.diagnostics.into_iter())
        .find(|d| d.code.as_deref() == Some("MER_OWN_003"))
        .unwrap_or_else(|| panic!("a MER_OWN_003 refusal in:\n{source}"));
    assert_eq!(diagnostic.message, message, "{diagnostic:#?}");
}

/// A default that hands a resource taken `out` to a parameter the callee
/// keeps gives the resource two owners, as passing it twice does.
#[test]
fn a_default_keeping_a_variable_taken_out_is_refused() {
    assert_default_kept_twice(
        r#"
fn main()
    var c = Conn(id: 3)
    let kept = store(c)
    println(f"{c.id} {kept[0].id}")
"#,
        "'c' is passed to 'store' as 'r', and the default of 'keep' passes it again, and 'store' keeps it: a resource has one owner\n  fix: pass 'keep' explicitly, a value of its own",
    );
}

#[test]
fn a_default_keeping_a_field_taken_out_is_refused() {
    assert_default_kept_twice(
        r#"
fn main()
    var s = Pair(c: Conn(id: 2))
    let kept = store(s.c)
    println(f"{s.c.id} {kept[0].id}")
"#,
        "'s.c' is passed to 'store' as 'r', and the default of 'keep' passes it again, and 'store' keeps it: a resource has one owner\n  fix: pass 'keep' explicitly, a value of its own",
    );
}

#[test]
fn a_kept_parameter_passed_explicitly_beside_a_resource_taken_out_runs() {
    assert_heap_guard_output(
        &format!(
            "{STORE}\n{}",
            r#"
fn main()
    var s = Pair(c: Conn(id: 2))
    var kept = store(s.c, Conn(id: 9))
    kept[0].id = 77
    println(f"{s.c.id} {kept[0].id}")
"#
        ),
        "closed 2\n60 77\nclosed 77\nclosed 60",
    );
}

const SAME_PLACE: &str = r#"
use system.io
use system.collections.list

class Point
    var n int

class Counter
    var n int

    fn absorb(other out Counter)
        other = Counter(n: other.n + self.n)

fn add(x out int, y int)
    x = x + y
"#;

fn with_same_place(program: &str) -> String {
    format!("{SAME_PLACE}\n{program}")
}

#[test]
fn a_variable_taken_out_beside_itself_lent_names_the_copy_first_repair() {
    assert_refused_exactly(
        &with_same_place(
            r#"
fn main()
    var a = 1
    add(a, a)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'x' is handed 'a', and the call also lends 'a', the same place: a call may not lend a value containing a place it writes",
        "pass 'a' once, and read what the other argument needs from it before the call: for a value, 'let before = a', then pass 'before' where 'a' is lent",
    );
}

#[test]
fn a_variable_lent_as_a_copy_under_its_own_name_runs() {
    assert_runs_with_output(
        &with_same_place(
            r#"
fn main()
    var a = 1
    let before = a
    add(a, before)
    println(f"{a}")
"#,
        ),
        "2",
    );
}

#[test]
fn a_field_taken_out_beside_itself_lent_is_refused() {
    assert_refused_exactly(
        &with_same_place(
            r#"
fn main()
    var p = Point(n: 1)
    add(p.n, p.n)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'x' is handed 'p.n', and the call also lends 'p.n', the same place: a call may not lend a value containing a place it writes",
        "pass 'p.n' once, and read what the other argument needs from it before the call: for a value, 'let before = p.n', then pass 'before' where 'p.n' is lent",
    );
}

#[test]
fn a_variable_taken_out_by_a_method_called_on_it_is_refused() {
    assert_refused_exactly(
        &with_same_place(
            r#"
fn main()
    var c = Counter(n: 1)
    c.absorb(c)
"#,
        ),
        "MER_TYP_066",
        "'out' parameter 'other' is handed 'c', and the call is made on 'c', the same place: a call may not lend a value containing a place it writes",
        "pass 'c' once, and read what the other argument needs from it before the call: for a value, 'let before = c', then pass 'before' where 'c' is lent",
    );
}

#[test]
fn two_elements_of_one_list_one_taken_out_still_run_with_the_pre_call_value() {
    assert_runs_with_output(
        &with_same_place(
            r#"
fn main()
    var v = List([1, 10])
    var i = 0
    add(v[i], v[i])
    add(v[0], v[1])
    println(f"{v[0]} {v[1]}")
"#,
        ),
        "12 10",
    );
}
