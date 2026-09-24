// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The element types the matrix places into every slot.
//!
//! Each type carries two ordered values `a < b`, their sum where the type has
//! one, and the exact text the matrix expects to read back for each. The
//! expected text is written here by hand, never learned by running Miri.

/// One value of an element type, as written and as observed.
pub struct Value {
    /// A Miri expression producing the value.
    pub expr: &'static str,
    /// What `ElementType::observe` prints for the value.
    pub shown: &'static str,
    /// What `f"{v}"` renders for the value, when the type renders at all.
    pub rendered: &'static str,
    /// A value sharing every bit of the low 64-bit word with this one and
    /// differing above it. Printing a value can hide a lost upper half, so a
    /// 128-bit read-back is also compared against this twin.
    pub twin: Option<&'static str>,
}

/// An element type: its spelling, its two values and what it supports.
pub struct ElementType {
    /// The stable token naming the type in a cell name.
    pub token: &'static str,
    /// The Miri spelling of the type.
    pub spelling: &'static str,
    pub a: Value,
    pub b: Value,
    /// `a + b`, for the types that define `+` (and so `+=`).
    pub sum: Option<Value>,
    /// Whether `==` is defined for the type.
    pub equatable: bool,
    /// Whether the type has an order, so a collection of it can be sorted.
    pub orderable: bool,
    /// Whether a value of the type may appear in string interpolation.
    pub renderable: bool,
    /// Whether the type may be a `Set` element or a `Map` key.
    pub keyable: bool,
    /// A Miri expression over `$r` turning a value into text without string
    /// interpolation of the value itself, so reading back a value is checked
    /// independently of how the type renders.
    pub observe: &'static str,
    /// Two values `==` calls equal, built separately — for a keyed collection
    /// to hold as one entry.
    pub equal_pair: (&'static str, &'static str),
    /// Top-level declarations the type's values and observation need.
    pub decls: &'static [Decl],
}

/// A named top-level declaration, emitted once per program that needs it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Decl {
    pub name: &'static str,
    pub text: &'static str,
}

const fn scalar(expr: &'static str) -> Value {
    Value {
        expr,
        shown: expr,
        rendered: expr,
        twin: None,
    }
}

const fn wide(expr: &'static str, twin: &'static str) -> Value {
    Value {
        expr,
        shown: expr,
        rendered: expr,
        twin: Some(twin),
    }
}

const fn named(expr: &'static str, shown: &'static str, rendered: &'static str) -> Value {
    Value {
        expr,
        shown,
        rendered,
        twin: None,
    }
}

const NUMBER: ElementType = ElementType {
    token: "",
    spelling: "",
    a: scalar("0"),
    b: scalar("0"),
    sum: None,
    equatable: true,
    orderable: true,
    renderable: true,
    keyable: true,
    observe: "f\"{$r}\"",
    equal_pair: ("0", "0"),
    decls: &[],
};

const fn number(
    token: &'static str,
    a: &'static str,
    b: &'static str,
    sum: &'static str,
) -> ElementType {
    ElementType {
        token,
        spelling: token,
        a: scalar(a),
        b: scalar(b),
        sum: Some(scalar(sum)),
        equal_pair: (a, a),
        ..NUMBER
    }
}

const fn wide_number(token: &'static str, a: Value, b: Value, sum: Value) -> ElementType {
    ElementType {
        token,
        spelling: token,
        equal_pair: (a.expr, a.expr),
        a,
        b,
        sum: Some(sum),
        ..NUMBER
    }
}

const POINT: Decl = Decl {
    name: "Pt",
    text: "struct Pt\n    x int\n    s String\n",
};

const BOX: Decl = Decl {
    name: "Bx",
    text: "class Bx\n    public x int\n    public s String\n\n    fn init(x int, s String)\n        self.x = x\n        self.s = s\n",
};

const SHAPE: Decl = Decl {
    name: "Sh",
    text: "enum Sh\n    Num(int)\n    Label(String)\n\n    fn equals(other Sh) bool\n        return show_sh(self) == show_sh(other)\n",
};

const SHOW_SHAPE: Decl = Decl {
    name: "show_sh",
    text: "fn show_sh(v Sh) String\n    match v\n        Sh.Num(n): f\"Num:{n}\"\n        Sh.Label(l): f\"Label:{l}\"\n",
};

const SHOW_OPTION: Decl = Decl {
    name: "show_opt",
    text: "fn show_opt(v Option<int>) String\n    match v\n        Some(n): f\"Some:{n}\"\n        None: \"None\"\n",
};

const ADDER: Decl = Decl {
    name: "adder",
    text: "fn adder(k int) fn(int) int\n    return fn(n int) int: n + k\n",
};

const VECTOR_IMPORT: Decl = Decl {
    name: "use system.gpu.vector",
    text: "use system.gpu.vector\n",
};

/// Every element type the matrix covers, in cell-name order.
pub const ELEMENT_TYPES: &[ElementType] = &[
    number("i8", "-5", "7", "2"),
    number("i16", "300", "1000", "1300"),
    number("i32", "70000", "80000", "150000"),
    number("int", "5000000000", "6000000000", "11000000000"),
    number("i64", "-5000000000", "6000000000", "1000000000"),
    // -(2^64 + 5) and 2^64 + 7: their low words are those of -5 and 7, and
    // their sum 2 shares its low word with 2^64 + 2.
    wide_number(
        "i128",
        wide("-18446744073709551621", "-5"),
        wide("18446744073709551623", "7"),
        wide("2", "18446744073709551618"),
    ),
    number("u8", "120", "130", "250"),
    number("u16", "20000", "40000", "60000"),
    number("u32", "1000000000", "3000000000", "4000000000"),
    number(
        "u64",
        "5000000000000000000",
        "10000000000000000000",
        "15000000000000000000",
    ),
    // 2^64 and 2^65: both have a zero low word.
    wide_number(
        "u128",
        wide("18446744073709551616", "0"),
        wide("36893488147419103232", "0"),
        wide("55340232221128654848", "0"),
    ),
    // A negative zero is `==` to zero but differs in its bytes.
    ElementType {
        equal_pair: ("0.0", "-0.0"),
        ..number("f32", "1.5", "2.25", "3.75")
    },
    ElementType {
        equal_pair: ("0.0", "-0.0"),
        ..number("float", "0.5", "1.25", "1.75")
    },
    ElementType {
        token: "bool",
        spelling: "bool",
        a: scalar("false"),
        b: scalar("true"),
        sum: None,
        equal_pair: ("true", "true"),
        ..NUMBER
    },
    ElementType {
        token: "string",
        spelling: "String",
        a: named("\"apple\"", "apple", "apple"),
        b: named("\"banana\"", "banana", "banana"),
        sum: Some(named("\"applebanana\"", "applebanana", "applebanana")),
        equal_pair: ("\"apple\"", "\"app\" + \"le\""),
        ..NUMBER
    },
    ElementType {
        token: "struct",
        spelling: "Pt",
        a: named("Pt(x: 1, s: \"a\")", "1:a", ""),
        b: named("Pt(x: 2, s: \"b\")", "2:b", ""),
        sum: None,
        orderable: false,
        renderable: false,
        observe: "f\"{$r.x}:{$r.s}\"",
        equal_pair: ("Pt(x: 1, s: \"a\")", "Pt(x: 1, s: \"a\" + \"\")"),
        decls: &[POINT],
        ..NUMBER
    },
    ElementType {
        token: "class",
        spelling: "Bx",
        a: named("Bx(x: 1, s: \"a\")", "1:a", ""),
        b: named("Bx(x: 2, s: \"b\")", "2:b", ""),
        sum: None,
        orderable: false,
        renderable: false,
        observe: "f\"{$r.x}:{$r.s}\"",
        // A class without `equals` is equal only to itself.
        equal_pair: ("Bx(x: 1, s: \"a\")", "a"),
        decls: &[BOX],
        ..NUMBER
    },
    ElementType {
        token: "enum",
        spelling: "Sh",
        a: named("Sh.Num(1)", "Num:1", "Num(1)"),
        b: named("Sh.Label(\"b\")", "Label:b", "Label(b)"),
        sum: None,
        orderable: false,
        observe: "show_sh($r)",
        equal_pair: ("Sh.Label(\"b\")", "Sh.Label(\"b\" + \"\")"),
        decls: &[SHAPE, SHOW_SHAPE],
        ..NUMBER
    },
    ElementType {
        token: "option_int",
        spelling: "Option<int>",
        a: named("Some(4)", "Some:4", "Some(4)"),
        b: named("None", "None", "None"),
        sum: None,
        orderable: false,
        observe: "show_opt($r)",
        equal_pair: ("Some(4)", "Some(4)"),
        decls: &[SHOW_OPTION],
        ..NUMBER
    },
    ElementType {
        token: "vec3_f32",
        spelling: "Vec3<f32>",
        a: named("Vec3<f32>(1.0, 2.0, 3.0)", "1.0 2.0 3.0", ""),
        b: named("Vec3<f32>(4.0, 5.0, 6.0)", "4.0 5.0 6.0", ""),
        sum: None,
        orderable: false,
        renderable: false,
        observe: "f\"{$r.x} {$r.y} {$r.z}\"",
        equal_pair: ("Vec3<f32>(1.0, 2.0, 3.0)", "Vec3<f32>(1.0, 2.0, 3.0)"),
        decls: &[VECTOR_IMPORT],
        ..NUMBER
    },
    ElementType {
        token: "closure",
        spelling: "fn(int) int",
        a: named("adder(10)", "11", ""),
        b: named("adder(20)", "21", ""),
        sum: None,
        equatable: false,
        orderable: false,
        renderable: false,
        keyable: false,
        observe: "f\"{$r(1)}\"",
        equal_pair: ("adder(10)", "a"),
        decls: &[ADDER],
    },
    ElementType {
        token: "array_string",
        spelling: "Array<String, 2>",
        a: named("[\"a\", \"b\"]", "ab", ""),
        b: named("[\"c\", \"d\"]", "cd", ""),
        sum: None,
        orderable: false,
        renderable: false,
        observe: "f\"{$r[0]}{$r[1]}\"",
        equal_pair: ("[\"a\", \"b\"]", "[\"a\", \"b\" + \"\"]"),
        ..NUMBER
    },
];
