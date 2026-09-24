// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The Miri source each cell compiles to.
//!
//! Every operation is a body over two parameters `a` and `b` of the element
//! type, plus `ea` and `eb`, second references to the same two values.
//! Storing a value can consume it — in a generic body, passing `a` to a
//! function moves it — so anything compared after the store is compared
//! against `ea` and `eb`. The body is then placed in the cell's context: a
//! plain function, a generic function, a generic class method, an inherited
//! method or a trait default. Every item a cell declares is numbered with the
//! cell's id, so an unlocated compiler error naming the item names the cell.

use super::cells::{Cell, Context, Operation, Slot};

/// The prefixes of every item name a cell declares; each is followed by the
/// cell's id.
pub const ITEM_PREFIXES: &[&str] = &[
    "cell", "drive", "get", "pick", "pass", "lit", "Cell", "Base", "Sub", "Op", "Impl", "HB", "HC",
    "H", "K",
];

/// The Miri source of one cell: its declarations and the call that runs it.
pub struct CellSource {
    /// Top-level items private to this cell.
    pub items: String,
    /// The expression, evaluated in the cell's driver, that runs the
    /// operation on the driver's `a`, `b`, `ea` and `eb`.
    pub call: String,
}

/// How the element type is spelled inside one cell's operation.
struct Spelling {
    /// The concrete spelling, or `T` in a generic context.
    t: &'static str,
    /// `<T>` in a generic context, empty otherwise.
    g: &'static str,
    id: usize,
}

/// Where a slot keeps the value and how it is reached.
struct Placement {
    /// Items the slot declares.
    items: String,
    /// Statements storing `a` in the slot.
    place: Vec<String>,
    /// The expression reading the stored value back.
    read: String,
    /// The place assigning to the slot writes, when it has one.
    lvalue: Option<String>,
}

/// The Miri source of `cell`, numbered `id` within its program.
pub fn cell_source(cell: &Cell, id: usize) -> CellSource {
    let generic = cell.context.is_generic();
    let spelling = Spelling {
        t: if generic { "T" } else { cell.ty.spelling },
        g: if generic { "<T>" } else { "" },
        id,
    };
    let (mut items, body) = operation_body(cell, &spelling);
    let returns = if cell.operation.counts() {
        "int"
    } else if cell.operation == Operation::Render {
        "String"
    } else {
        spelling.t
    };
    let call = wrap_in_context(cell, id, returns, &body, &mut items);
    CellSource { items, call }
}

/// The items and statements (ending in `return`) of the cell's operation.
fn operation_body(cell: &Cell, s: &Spelling) -> (String, Vec<String>) {
    if let Some(found) = slot_specific_body(cell, s) {
        return found;
    }
    let Placement {
        items,
        place: mut lines,
        read,
        lvalue,
    } = placement(cell.slot, s);
    let lvalue = lvalue.unwrap_or_default();
    match cell.operation {
        Operation::StoreRead => lines.push(format!("return {read}")),
        Operation::Overwrite => lines.extend([format!("{lvalue} = b"), format!("return {read}")]),
        Operation::WriteLiteral => lines.extend([
            format!("{lvalue} = {}", cell.ty.b.expr),
            format!("return {read}"),
        ]),
        Operation::CompoundAssign => {
            lines.extend([format!("{lvalue} += b"), format!("return {read}")])
        }
        Operation::Equality => {
            lines.push(format!("let got = {read}"));
            lines.extend(count_lines("got == ea", "got == eb"));
        }
        Operation::Render => lines.push(format!("return f\"{{{read}}}\"")),
        Operation::Contains
        | Operation::Dedup
        | Operation::Sort
        | Operation::Pop
        | Operation::First
        | Operation::Last
        | Operation::RemoveAt
        | Operation::Reversed
        | Operation::Construct
        | Operation::Get
        | Operation::Add => unreachable!(
            "`{}` exists only in the slots `slot_specific_body` writes",
            cell.name
        ),
    }
    (items, lines)
}

/// Statements counting two tests into `r` and returning it: 1 for the first,
/// 2 for the second.
fn count_lines(first: &str, second: &str) -> Vec<String> {
    vec![
        "var r = 0".to_string(),
        format!("if {first}"),
        "    r = r + 1".to_string(),
        format!("if {second}"),
        "    r = r + 2".to_string(),
        "return r".to_string(),
    ]
}

fn owned_lines(text: &[&str]) -> Vec<String> {
    text.iter().map(|line| line.to_string()).collect()
}

/// Where each slot keeps `a`.
fn placement(slot: Slot, s: &Spelling) -> Placement {
    let (t, g, id) = (s.t, s.g, s.id);
    let simple = |place: Vec<String>, read: &str, lvalue: Option<&str>| Placement {
        items: String::new(),
        place,
        read: read.to_string(),
        lvalue: lvalue.map(str::to_string),
    };
    match slot {
        Slot::Local => simple(vec![format!("var x {t} = a")], "x", Some("x")),
        Slot::StructField => Placement {
            items: format!("struct H{id}{g}\n    v {t}\n"),
            ..simple(vec![format!("var h = H{id}{g}(v: a)")], "h.v", Some("h.v"))
        },
        Slot::ClassField => Placement {
            items: holder_class(&format!("H{id}{g}"), t),
            ..simple(vec![format!("var h = H{id}{g}(v: a)")], "h.v", Some("h.v"))
        },
        Slot::InheritedField => Placement {
            items: inherited_holder(s),
            ..simple(
                vec![format!("var h = HC{id}{g}(v: a, n: 7, s: \"x\" + \"y\")")],
                "h.v",
                Some("h.v"),
            )
        },
        Slot::EnumPayload => Placement {
            items: enum_holder(s),
            ..simple(
                vec![format!("let e H{id}{g} = H{id}.Has(a, 42)")],
                &format!("get{id}(e, b)"),
                None,
            )
        },
        Slot::ListElement => simple(
            vec![format!("var l = List<{t}>()"), "l.push(a)".into()],
            "l[0]",
            Some("l[0]"),
        ),
        Slot::ArrayElement => simple(vec!["var arr = [a]".into()], "arr[0]", Some("arr[0]")),
        Slot::SetElement => simple(
            vec![format!("var s = Set<{t}>()"), "s.add(a)".into()],
            "s.element_at(0)",
            None,
        ),
        Slot::MapKey => simple(
            vec![format!("var m = Map<{t}, int>()"), "m[a] = 1".into()],
            "m.element_at(0)",
            None,
        ),
        Slot::MapValue => simple(
            vec![format!("var m = Map<int, {t}>()"), "m[1] = a".into()],
            "m[1]",
            Some("m[1]"),
        ),
        Slot::Parameter => simple(Vec::new(), "a", None),
        Slot::Return => Placement {
            items: pass_function(s),
            ..simple(vec![format!("let x = pass{id}(a)")], "x", None)
        },
        Slot::ClosureCapture => simple(vec![format!("let c = fn() {t}: a")], "c()", None),
        Slot::GenericField => Placement {
            items: format!("struct H{id}<U>\n    v U\n"),
            ..simple(
                vec![format!("var h = H{id}<{t}>(v: a)")],
                "h.v",
                Some("h.v"),
            )
        },
    }
}

fn holder_class(name: &str, t: &str) -> String {
    format!("class {name}\n    public v {t}\n\n    fn init(v {t})\n        self.v = v\n")
}

/// A generic-or-not base class holding the value first, then a scalar and a
/// managed field, and a subclass reaching it through `extends`.
fn inherited_holder(s: &Spelling) -> String {
    let (t, g, id) = (s.t, s.g, s.id);
    format!(
        "class HB{id}{g}\n    public v {t}\n    public n int\n    public s String\n\n    \
         fn init(v {t}, n int, s String)\n        self.v = v\n        self.n = n\n        \
         self.s = s\n\nclass HC{id}{g} extends HB{id}{g}\n    \
         fn init(v {t}, n int, s String)\n        super.init(v, n, s)\n"
    )
}

/// An enum whose variant carries the value beside a marker, and a getter
/// that hands the value back only when the marker survived beside it.
fn enum_holder(s: &Spelling) -> String {
    let (t, g, id) = (s.t, s.g, s.id);
    format!(
        "enum H{id}{g}\n    Has({t}, int)\n    Nothing\n\n\
         fn pick{id}{g}(v {t}, d {t}, intact bool) {t}\n    if intact\n        return v\n    return d\n\n\
         fn get{id}{g}(e H{id}{g}, d {t}) {t}\n    match e\n        \
         H{id}.Has(v, n): pick{id}(v, d, n == 42)\n        H{id}.Nothing: d\n"
    )
}

fn pass_function(s: &Spelling) -> String {
    let (t, g, id) = (s.t, s.g, s.id);
    format!("fn pass{id}{g}(p {t}) {t}\n    return p\n")
}

/// The bodies of operations only some slots have, and of operations a slot
/// performs its own way.
fn slot_specific_body(cell: &Cell, s: &Spelling) -> Option<(String, Vec<String>)> {
    let found = match cell.operation {
        Operation::Contains => contains_body(cell.slot, s),
        Operation::Dedup => dedup_body(cell.slot, s),
        Operation::Sort
        | Operation::Pop
        | Operation::First
        | Operation::Last
        | Operation::RemoveAt
        | Operation::Reversed
        | Operation::Construct => sequence_body(cell.slot, cell.operation, s),
        Operation::Get => Some(vec![
            format!("var m = Map<int, {}>()", s.t),
            "m[1] = a".into(),
            "return m.get(1) ?? eb".into(),
        ]),
        Operation::Add => {
            let id = s.id;
            return Some((
                pass_function(s),
                vec![format!("return pass{id}(a) + pass{id}(b)")],
            ));
        }
        Operation::WriteLiteral if cell.slot == Slot::Return => {
            let (t, id) = (s.t, s.id);
            return Some((
                format!("fn lit{id}() {t}\n    return {}\n", cell.ty.b.expr),
                vec![format!("return lit{id}()")],
            ));
        }
        Operation::Overwrite | Operation::CompoundAssign if cell.slot == Slot::ClosureCapture => {
            return Some(captured_write_body(cell.operation, s));
        }
        Operation::StoreRead
        | Operation::Overwrite
        | Operation::WriteLiteral
        | Operation::Equality
        | Operation::CompoundAssign
        | Operation::Render => None,
    };
    found.map(|body| (String::new(), body))
}

fn contains_body(slot: Slot, s: &Spelling) -> Option<Vec<String>> {
    let t = s.t;
    let (mut body, method) = match slot {
        Slot::ListElement => (
            vec![format!("var l = List<{t}>()"), "l.push(a)".into()],
            "l.contains",
        ),
        Slot::ArrayElement => (vec!["var arr = [a]".into()], "arr.contains"),
        Slot::SetElement => (
            vec![format!("var s = Set<{t}>()"), "s.add(a)".into()],
            "s.contains",
        ),
        Slot::MapKey => (
            vec![format!("var m = Map<{t}, int>()"), "m[a] = 1".into()],
            "m.contains_key",
        ),
        _ => return None,
    };
    body.extend(count_lines(
        &format!("{method}(ea)"),
        &format!("{method}(eb)"),
    ));
    Some(body)
}

fn dedup_body(slot: Slot, s: &Spelling) -> Option<Vec<String>> {
    let t = s.t;
    match slot {
        Slot::SetElement => Some(vec![
            format!("var s = Set<{t}>()"),
            "s.add(a)".into(),
            "s.add(b)".into(),
            "return s.length()".into(),
        ]),
        Slot::MapKey => Some(vec![
            format!("var m = Map<{t}, int>()"),
            "m[a] = 1".into(),
            "m[b] = 2".into(),
            "return m.length()".into(),
        ]),
        _ => None,
    }
}

/// The ordered-collection operations, on a list or an array holding `a` then
/// `b` (or `b` then `a`, for sorting).
fn sequence_body(slot: Slot, operation: Operation, s: &Spelling) -> Option<Vec<String>> {
    let t = s.t;
    let list = format!("var l = List<{t}>()");
    let body = match (slot, operation) {
        (Slot::ListElement, Operation::Sort) => vec![
            list,
            "l.push(b)".into(),
            "l.push(a)".into(),
            "l.sort()".into(),
            "return l[0]".into(),
        ],
        (Slot::ArrayElement, Operation::Sort) => {
            owned_lines(&["var arr = [b, a]", "arr.sort()", "return arr[0]"])
        }
        (Slot::ListElement, Operation::Construct) => {
            vec![format!("var l = List<{t}>([a, b])"), "return l[0]".into()]
        }
        (Slot::ArrayElement, Operation::Construct) => vec![
            format!("var arr = Array<{t}, 2>(a, b)"),
            "return arr[0]".into(),
        ],
        (Slot::ListElement, _) => {
            let tail = match operation {
                Operation::Pop => "return l.pop() ?? ea",
                Operation::First => "return l.first() ?? eb",
                Operation::Last => "return l.last() ?? ea",
                Operation::RemoveAt => "return l.remove_at(0) ?? eb",
                Operation::Reversed => "return l.reversed()[0]",
                _ => return None,
            };
            vec![
                list,
                "l.push(a)".into(),
                "l.push(b)".into(),
                tail.to_string(),
            ]
        }
        (Slot::ArrayElement, Operation::First) => {
            owned_lines(&["var arr = [a, b]", "return arr.first() ?? eb"])
        }
        (Slot::ArrayElement, Operation::Last) => {
            owned_lines(&["var arr = [a, b]", "return arr.last() ?? ea"])
        }
        _ => return None,
    };
    Some(body)
}

/// A write through an object a closure captured: the closure stores `b` in
/// (or adds `b` to) a field of a class it captures, and the caller reads the
/// field back after calling it.
fn captured_write_body(operation: Operation, s: &Spelling) -> (String, Vec<String>) {
    let (g, id) = (s.g, s.id);
    let write = if operation == Operation::CompoundAssign {
        "    h.v += b"
    } else {
        "    h.v = b"
    };
    (
        holder_class(&format!("K{id}{g}"), s.t),
        vec![
            format!("var h = K{id}{g}(v: a)"),
            "let f = fn()".into(),
            write.to_string(),
            "f()".into(),
            "return h.v".into(),
        ],
    )
}

/// Emits the function, class or trait holding `body` for the cell's context
/// into `items`, and returns the call that runs it.
fn wrap_in_context(
    cell: &Cell,
    id: usize,
    returns: &str,
    body: &[String],
    items: &mut String,
) -> String {
    let concrete = cell.ty.spelling;
    let t = if cell.context.is_generic() {
        "T"
    } else {
        concrete
    };
    let params = format!("(a {t}, b {t}, ea {t}, eb {t}) {returns}");
    let (header, depth, call, trailer) = match cell.context {
        Context::Monomorphic => (
            format!("fn cell{id}{params}\n"),
            1,
            format!("cell{id}(a, b, ea, eb)"),
            String::new(),
        ),
        Context::GenericFunction => (
            format!("fn cell{id}<T>{params}\n"),
            1,
            format!("cell{id}(a, b, ea, eb)"),
            String::new(),
        ),
        Context::GenericClassMethod => (
            format!("class Cell{id}<T>\n    fn run{params}\n"),
            2,
            format!("Cell{id}<{concrete}>().run(a, b, ea, eb)"),
            String::new(),
        ),
        Context::InheritedMethod => (
            format!("class Base{id}<T>\n    fn run{params}\n"),
            2,
            format!("Sub{id}().run(a, b, ea, eb)"),
            format!("class Sub{id} extends Base{id}<{concrete}>\n"),
        ),
        Context::TraitDefault => (
            format!("trait Op{id}<T>\n    fn run{params}\n"),
            2,
            format!("Impl{id}().run(a, b, ea, eb)"),
            format!("class Impl{id} implements Op{id}<{concrete}>\n"),
        ),
    };
    items.push_str(&header);
    let indent = "    ".repeat(depth);
    for line in body {
        items.push_str(&indent);
        items.push_str(line);
        items.push('\n');
    }
    items.push('\n');
    items.push_str(&trailer);
    call
}
