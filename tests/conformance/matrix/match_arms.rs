// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Match cells: every scrutinee kind under every arm shape, matched at every
//! variant, checked by the arm the value reaches.
//!
//! An arm that tests a payload — through a guard, a literal, or just by binding
//! it — leaves the variants it does not cover to the arms after it. A defect in
//! how those variants are routed shows only at the variant no arm names, and
//! only once the arm before it binds the payload that variant does not have, so
//! each cell runs one variant through one arm shape and compares the arm it
//! reached against the one the language requires.

use super::ownership::{execute, first_line};

/// The type a match dispatches on.
#[derive(Clone, Copy)]
enum Scrutinee {
    OptionalInt,
    OptionalString,
    Result,
    /// A user enum with one payload variant and one without.
    UserEnum,
}

impl Scrutinee {
    const ALL: [Scrutinee; 4] = [
        Scrutinee::OptionalInt,
        Scrutinee::OptionalString,
        Scrutinee::Result,
        Scrutinee::UserEnum,
    ];

    fn token(self) -> &'static str {
        match self {
            Scrutinee::OptionalInt => "optional_int",
            Scrutinee::OptionalString => "optional_string",
            Scrutinee::Result => "result",
            Scrutinee::UserEnum => "user_enum",
        }
    }

    fn type_name(self) -> &'static str {
        match self {
            Scrutinee::OptionalInt => "int?",
            Scrutinee::OptionalString => "String?",
            Scrutinee::Result => "Result<int, String>",
            Scrutinee::UserEnum => "Shape",
        }
    }

    /// The payload variant's pattern with `inner` in its payload position.
    fn payload_pattern(self, inner: &str) -> String {
        match self {
            Scrutinee::OptionalInt | Scrutinee::OptionalString => format!("Some({inner})"),
            Scrutinee::Result => format!("Result.Ok({inner})"),
            Scrutinee::UserEnum => format!("Shape.Circle({inner})"),
        }
    }

    /// The pattern for the variant the payload arm does not cover.
    fn other_pattern(self) -> &'static str {
        match self {
            Scrutinee::OptionalInt | Scrutinee::OptionalString => "None",
            Scrutinee::Result => "Result.Err(_)",
            Scrutinee::UserEnum => "Shape.Empty",
        }
    }

    /// The payload the arms single out, as a literal.
    fn chosen_payload(self) -> &'static str {
        match self {
            Scrutinee::OptionalString => "\"a\"",
            Scrutinee::OptionalInt | Scrutinee::Result | Scrutinee::UserEnum => "1",
        }
    }

    /// A payload other than the chosen one, as a literal.
    fn other_payload(self) -> &'static str {
        match self {
            Scrutinee::OptionalString => "\"b\"",
            Scrutinee::OptionalInt | Scrutinee::Result | Scrutinee::UserEnum => "2",
        }
    }

    /// An expression of this type at `variant`.
    fn value(self, variant: Variant) -> String {
        let payload = match variant {
            Variant::Chosen => self.chosen_payload(),
            Variant::OtherPayload => self.other_payload(),
            Variant::Other => {
                return match self {
                    Scrutinee::OptionalInt | Scrutinee::OptionalString => "None".to_string(),
                    Scrutinee::Result => "Result.Err(\"e\")".to_string(),
                    Scrutinee::UserEnum => "Shape.Empty".to_string(),
                }
            }
        };
        match self {
            Scrutinee::OptionalInt | Scrutinee::OptionalString => payload.to_string(),
            Scrutinee::Result => format!("Result.Ok({payload})"),
            Scrutinee::UserEnum => format!("Shape.Circle({payload})"),
        }
    }
}

/// The arms a match is written with. Each arm yields its number: 1 for the
/// payload arm, 2 for an explicit second payload arm, 3 for the rest.
#[derive(Clone, Copy)]
enum ArmShape {
    /// A guarded payload arm, then `_`.
    GuardThenWildcard,
    /// A guarded payload arm, then the payload variant and the other variant
    /// spelled out.
    GuardThenExplicit,
    /// A payload arm testing a literal, then `_`.
    LiteralThenWildcard,
    /// A payload arm binding the payload, then a binding catch-all.
    BindingThenCatchAll,
}

impl ArmShape {
    const ALL: [ArmShape; 4] = [
        ArmShape::GuardThenWildcard,
        ArmShape::GuardThenExplicit,
        ArmShape::LiteralThenWildcard,
        ArmShape::BindingThenCatchAll,
    ];

    fn token(self) -> &'static str {
        match self {
            ArmShape::GuardThenWildcard => "guard_then_wildcard",
            ArmShape::GuardThenExplicit => "guard_then_explicit",
            ArmShape::LiteralThenWildcard => "literal_then_wildcard",
            ArmShape::BindingThenCatchAll => "binding_then_catch_all",
        }
    }

    fn arms(self, scrutinee: Scrutinee) -> Vec<String> {
        let chosen = scrutinee.chosen_payload();
        let guarded = format!("{} if c == {chosen}: 1", scrutinee.payload_pattern("c"));
        match self {
            ArmShape::GuardThenWildcard => vec![guarded, "_: 3".into()],
            ArmShape::GuardThenExplicit => vec![
                guarded,
                format!("{}: 2", scrutinee.payload_pattern("_")),
                format!("{}: 3", scrutinee.other_pattern()),
            ],
            ArmShape::LiteralThenWildcard => {
                vec![
                    format!("{}: 1", scrutinee.payload_pattern(chosen)),
                    "_: 3".into(),
                ]
            }
            ArmShape::BindingThenCatchAll => {
                vec![
                    format!("{}: 1", scrutinee.payload_pattern("_c")),
                    "_rest: 3".into(),
                ]
            }
        }
    }

    /// The arm `variant` must reach.
    fn expected_arm(self, variant: Variant) -> u8 {
        match (self, variant) {
            (_, Variant::Chosen) => 1,
            (ArmShape::BindingThenCatchAll, Variant::OtherPayload) => 1,
            (ArmShape::GuardThenExplicit, Variant::OtherPayload) => 2,
            (
                ArmShape::GuardThenWildcard | ArmShape::LiteralThenWildcard,
                Variant::OtherPayload,
            ) => 3,
            (_, Variant::Other) => 3,
        }
    }
}

/// The value a cell matches.
#[derive(Clone, Copy)]
enum Variant {
    /// The payload variant, carrying the payload the first arm singles out.
    Chosen,
    /// The payload variant, carrying another payload.
    OtherPayload,
    /// The variant the payload arm does not name: `None`, `Err`, `Empty`.
    Other,
}

impl Variant {
    const ALL: [Variant; 3] = [Variant::Chosen, Variant::OtherPayload, Variant::Other];

    fn token(self) -> &'static str {
        match self {
            Variant::Chosen => "chosen",
            Variant::OtherPayload => "other_payload",
            Variant::Other => "other_variant",
        }
    }
}

/// One match cell: a named program and the arm it must print.
pub struct MatchCell {
    pub name: String,
    source: String,
    expected: u8,
}

/// Every match cell, in a stable order.
pub fn match_cells() -> Vec<MatchCell> {
    let mut cells = Vec::new();
    for scrutinee in Scrutinee::ALL {
        for shape in ArmShape::ALL {
            for variant in Variant::ALL {
                cells.push(MatchCell {
                    name: format!(
                        "match/{}/{}/{}",
                        scrutinee.token(),
                        shape.token(),
                        variant.token()
                    ),
                    source: program(scrutinee, shape, variant),
                    expected: shape.expected_arm(variant),
                });
            }
        }
    }
    cells
}

/// The whole program for one cell: the enum it may need, the match in a
/// function taking the scrutinee, and a driver printing `begin` and `end`
/// around the arm reached.
fn program(scrutinee: Scrutinee, shape: ArmShape, variant: Variant) -> String {
    let arms: String = shape
        .arms(scrutinee)
        .iter()
        .map(|arm| format!("        {arm}\n"))
        .collect();
    format!(
        "enum Shape\n    Circle(int)\n    Empty\n\n\
         fn pick(x {}) int\n    match x\n{arms}\n\
         fn main()\n    println(\"begin\")\n    let reached = pick({})\n    \
         println(f\"{{reached}}\")\n    println(\"end\")\n",
        scrutinee.type_name(),
        scrutinee.value(variant),
    )
}

/// The red cells among `cells`, each with one line of evidence.
pub fn red_match_cells(cells: &[MatchCell]) -> Vec<(String, String)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = cells
            .iter()
            .map(|cell| scope.spawn(move || judge(cell).map(|reason| (cell.name.clone(), reason))))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().expect("a match cell panicked"))
            .collect()
    })
}

/// Why `cell` is red, or `None` when it is green.
fn judge(cell: &MatchCell) -> Option<String> {
    let (exit_ok, stdout, stderr) = execute(&cell.source);
    let clean =
        exit_ok && !stderr.contains("MIRI_LEAK_CHECK:") && !stderr.contains("MIRI_HEAP_GUARD:");
    if !clean {
        return Some(first_line(&format!("{stdout}{stderr}")));
    }
    let printed: Vec<&str> = stdout
        .lines()
        .skip_while(|line| *line != "begin")
        .skip(1)
        .take_while(|line| *line != "end")
        .collect();
    let expected = cell.expected.to_string();
    if printed == [expected.as_str()] {
        None
    } else {
        Some(format!(
            "reached `{}`, expected arm {expected}",
            printed.join(" | ")
        ))
    }
}
