// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A match on an enum held as its bytes — an element, a field, a local —
//! copies the value it matches, and code generation lets that copy name the
//! bytes it was copied from wherever nothing can change them before the copy
//! is read. Each arm binds its payload before its body runs, so a body that
//! overwrites the element, the index or the local it matched still sees the
//! payload the value had when it was matched. Each program runs under the
//! heap guard.

use super::utils::*;

const INSTRS: &str = "
use system.io
use system.collections.list

enum Instr
    Set(int, int)
    Swap(int, int)
    Halt
";

fn with_instrs(body: &str) -> String {
    format!("{INSTRS}{body}")
}

#[test]
fn test_an_arm_overwriting_the_matched_element_keeps_its_bindings() {
    assert_heap_guard_output(
        &with_instrs(
            "
fn main()
    var code = List([Instr.Set(1, 2), Instr.Swap(3, 4), Instr.Halt])
    var pc = 0
    var running = true
    while running
        match code[pc]
            Instr.Set(a, b)
                code[pc] = Instr.Swap(a + 10, b + 10)
                println(f\"set {a} {b}\")
            Instr.Swap(a, b)
                code[pc] = Instr.Halt
                pc += 1
                println(f\"swap {a} {b}\")
            Instr.Halt: running = false
",
        ),
        "set 1 2\nswap 11 12\nswap 3 4\n",
    );
}

#[test]
fn test_an_arm_changing_the_index_keeps_its_bindings() {
    assert_heap_guard_output(
        &with_instrs(
            "
fn main()
    let code = List([Instr.Set(1, 2), Instr.Set(3, 4)])
    var pc = 0
    match code[pc]
        Instr.Set(a, b)
            pc = 1
            println(f\"{a} {b} {pc}\")
        _: println(\"other\")
",
        ),
        "1 2 1\n",
    );
}

/// A failing guard leaves for the next arm, which binds the same element
/// again.
#[test]
fn test_a_guard_reading_a_list_falls_through_to_the_next_arm() {
    assert_heap_guard_output(
        &with_instrs(
            "
fn main()
    let code = List([Instr.Set(1, 2), Instr.Set(2, 4), Instr.Halt])
    let limits = List([5, 1, 0])
    var pc = 0
    while pc < 3
        match code[pc]
            Instr.Set(a, b) if a < limits[pc]: println(f\"under {a} {b}\")
            Instr.Set(a, b): println(f\"over {a} {b}\")
            _: println(\"halt\")
        pc += 1
",
        ),
        "under 1 2\nover 2 4\nhalt\n",
    );
}

#[test]
fn test_a_match_on_a_struct_field_binds_its_payload() {
    assert_heap_guard_output(
        &with_instrs(
            "
struct Step
    id int
    instr Instr

fn main()
    var step = Step(id: 1, instr: Instr.Swap(3, 4))
    match step.instr
        Instr.Swap(a, b)
            step.instr = Instr.Halt
            println(f\"{step.id} {a} {b}\")
        _: println(\"other\")
    match step.instr
        Instr.Halt: println(\"halted\")
        _: println(\"other\")
",
        ),
        "1 3 4\nhalted\n",
    );
}

#[test]
fn test_a_match_on_a_class_field_binds_its_payload() {
    assert_heap_guard_output(
        &with_instrs(
            "
class Machine
    var current Instr

    fn step() int
        match self.current
            Instr.Set(a, b)
                self.current = Instr.Halt
                return a + b
            Instr.Swap(a, b): return a - b
            Instr.Halt: return 0

fn main()
    let m = Machine(current: Instr.Set(3, 4))
    println(f\"{m.step()} {m.step()}\")
",
        ),
        "7 0\n",
    );
}

#[test]
fn test_a_match_on_a_local_the_arm_reassigns_keeps_its_bindings() {
    assert_heap_guard_output(
        &with_instrs(
            "
fn main()
    var instr = Instr.Set(5, 6)
    match instr
        Instr.Set(a, b)
            instr = Instr.Swap(a + 1, b + 1)
            println(f\"{a} {b}\")
        _: println(\"other\")
    match instr
        Instr.Swap(a, b): println(f\"{a} {b}\")
        _: println(\"other\")
",
        ),
        "5 6\n6 7\n",
    );
}

#[test]
fn test_nested_patterns_and_alternatives_on_an_element_bind_in_place() {
    assert_heap_guard_output(
        &with_instrs(
            "
enum Slot
    Empty
    Full(Instr, int)

fn describe(slots [Slot], i int) String
    match slots[i]
        Slot.Full(Instr.Set(a, 2), n): f\"set two {a} {n}\"
        Slot.Full(Instr.Set(a, _), n) | Slot.Full(Instr.Swap(a, _), n): f\"first {a} {n}\"
        Slot.Full(Instr.Halt, n): f\"halt {n}\"
        Slot.Empty: \"empty\"

fn main()
    let slots = List([
        Slot.Full(Instr.Set(1, 2), 10),
        Slot.Full(Instr.Set(3, 4), 11),
        Slot.Full(Instr.Swap(5, 6), 12),
        Slot.Full(Instr.Halt, 13),
        Slot.Empty
    ])
    var i = 0
    while i < slots.length()
        println(describe(slots, i))
        i += 1
",
        ),
        "set two 1 10\nfirst 3 11\nfirst 5 12\nhalt 13\nempty\n",
    );
}

/// A copy read after its source is overwritten, or after the index naming
/// its source changes, keeps the value it copied.
#[test]
fn test_a_copy_keeps_its_value_when_its_source_changes_before_it_is_read() {
    assert_heap_guard_output(
        &with_instrs(
            "
struct Step
    instr Instr

fn main()
    var code = List([Instr.Set(1, 2), Instr.Swap(3, 4)])
    var pc = 0
    let first = code[pc]
    code[0] = Instr.Halt
    pc = 1
    let second = code[0]
    var step = Step(instr: Instr.Swap(5, 6))
    let held = step.instr
    step.instr = Instr.Halt
    for instr in [first, second, held]
        match instr
            Instr.Set(a, b): println(f\"set {a} {b}\")
            Instr.Swap(a, b): println(f\"swap {a} {b}\")
            Instr.Halt: println(\"halt\")
",
        ),
        "set 1 2\nhalt\nswap 5 6\n",
    );
}

#[test]
fn test_a_match_on_an_element_past_the_end_stops_the_program() {
    assert_runtime_error(
        &with_instrs(
            "
fn main()
    let code = List([Instr.Halt])
    var pc = 3
    match code[pc]
        Instr.Set(a, b): println(f\"{a} {b}\")
        _: println(\"other\")
",
        ),
        "index out of bounds: the len is 1 but the index is 3",
    );
}
