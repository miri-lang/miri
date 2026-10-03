// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Resource cells: values whose type declares a drop hook, moved and released
//! along every ownership path, checked by when the hook runs.
//!
//! A resource moves on assignment, so it cannot occupy the value matrix, which
//! stores a value and compares its read-back against a second reference to it.
//! These cells instead run one small program each and compare every line it
//! prints, the hook's sentinel included, against the exact sequence the
//! language requires: the hook runs once, at the point the value's last owner
//! lets it go, and never again.

use crate::utils::miri_cmd;
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::NamedTempFile;

/// A program that runs longer than this is treated as hung.
const TIMEOUT: Duration = Duration::from_secs(300);

/// A kind of named type a drop hook can be declared on. A struct holds data
/// only and declares none.
#[derive(Clone, Copy)]
pub enum Resource {
    Class,
    Enum,
}

impl Resource {
    const ALL: [Resource; 2] = [Resource::Class, Resource::Enum];

    fn token(self) -> &'static str {
        match self {
            Resource::Class => "class",
            Resource::Enum => "enum",
        }
    }

    /// The declaration of `Res`, whose hook prints `drop <id>`, and of the
    /// trait `Tr` it implements where its kind can implement one.
    fn declaration(self) -> &'static str {
        match self {
            Resource::Class => {
                "trait Tr\n    fn tag() int\n        return 1\n\n\
                 class Res implements Tr\n    public var id int\n    fn init(id int)\n        self.id = id\n    \
                 fn drop(self)\n        println(f\"drop {self.id}\")\n"
            }
            Resource::Enum => {
                "enum Res\n    Tag(int)\n\n    fn drop(self)\n        match self\n            \
                 Res.Tag(n): println(f\"drop {n}\")\n"
            }
        }
    }

    /// A Miri expression building the resource numbered `id`.
    fn make(self, id: u32) -> String {
        match self {
            Resource::Class => format!("Res({id})"),
            Resource::Enum => format!("Res.Tag({id})"),
        }
    }

    fn implements_a_trait(self) -> bool {
        matches!(self, Resource::Class)
    }
}

/// A path a resource's ownership can take.
#[derive(Clone, Copy)]
enum Path {
    /// Its only binding goes out of scope.
    ScopeExit,
    /// It is moved into a second binding, which then goes out of scope.
    MoveIntoSlot,
    /// Its hook is called by name; the scope's end must not run it again.
    DropCall,
    /// It is held at a trait type, which then goes out of scope.
    ReleaseThroughTrait,
    /// It is handed to a consuming function on every pass of a loop, which
    /// uses it again after the first pass moved it.
    ConsumeInLoop,
}

impl Path {
    const ALL: [Path; 5] = [
        Path::ScopeExit,
        Path::MoveIntoSlot,
        Path::DropCall,
        Path::ReleaseThroughTrait,
        Path::ConsumeInLoop,
    ];

    fn token(self) -> &'static str {
        match self {
            Path::ScopeExit => "scope_exit",
            Path::MoveIntoSlot => "move_into_slot",
            Path::DropCall => "drop_call",
            Path::ReleaseThroughTrait => "release_through_trait",
            Path::ConsumeInLoop => "consume_in_loop",
        }
    }

    /// The statements exercising the path on `made`, one per line, and what
    /// the program must do: print exactly the lines given, or be refused with
    /// the code given.
    fn body(self, made: &str) -> (Vec<String>, Expected) {
        let printed =
            |lines: &[&str]| Expected::Prints(lines.iter().map(|l| l.to_string()).collect());
        match self {
            Path::ScopeExit => (
                vec![
                    "if true".into(),
                    format!("    let r = {made}"),
                    "    println(\"inside\")".into(),
                ],
                printed(&["inside", "drop 1"]),
            ),
            Path::MoveIntoSlot => (
                vec![
                    "if true".into(),
                    format!("    let r = {made}"),
                    "    var s = r".into(),
                    "    println(\"moved\")".into(),
                ],
                printed(&["moved", "drop 1"]),
            ),
            Path::DropCall => (
                vec![
                    "if true".into(),
                    format!("    let r = {made}"),
                    "    r.drop()".into(),
                    "    println(\"after\")".into(),
                ],
                printed(&["drop 1", "after"]),
            ),
            Path::ReleaseThroughTrait => (
                vec![
                    "if true".into(),
                    format!("    let t Tr = {made}"),
                    "    println(\"held\")".into(),
                ],
                printed(&["held", "drop 1"]),
            ),
            Path::ConsumeInLoop => (
                vec![
                    format!("let r = {made}"),
                    "var i = 0".into(),
                    "while i < 2".into(),
                    "    consume(r)".into(),
                    "    i = i + 1".into(),
                ],
                Expected::Refused("MER_OWN_003"),
            ),
        }
    }
}

/// Where the path is exercised.
#[derive(Clone, Copy)]
enum Place {
    /// Directly in `main`.
    Mono,
    /// In the body of a generic function `main` calls.
    GenericFn,
}

impl Place {
    const ALL: [Place; 2] = [Place::Mono, Place::GenericFn];

    fn token(self) -> &'static str {
        match self {
            Place::Mono => "mono",
            Place::GenericFn => "generic_fn",
        }
    }
}

/// What a resource cell's program must do.
#[derive(Clone)]
enum Expected {
    /// Print exactly these lines between the driver's `begin` and `end`.
    Prints(Vec<String>),
    /// Be refused with this diagnostic code.
    Refused(&'static str),
}

/// One resource cell: a named program and what it must do.
pub struct ResourceCell {
    pub name: String,
    source: String,
    expected: Expected,
}

/// Every resource cell, in a stable order.
pub fn resource_cells() -> Vec<ResourceCell> {
    let mut cells = Vec::new();
    for resource in Resource::ALL {
        for path in Path::ALL {
            if matches!(path, Path::ReleaseThroughTrait) && !resource.implements_a_trait() {
                continue;
            }
            for place in Place::ALL {
                let (statements, expected) = path.body(&resource.make(1));
                cells.push(ResourceCell {
                    name: format!(
                        "resource/{}/{}/{}",
                        resource.token(),
                        path.token(),
                        place.token()
                    ),
                    source: program(resource, place, &statements),
                    expected,
                });
            }
        }
    }
    cells
}

/// The whole program for one cell: the declarations, a consuming function,
/// and a driver printing `begin` and `end` around the path's statements.
fn program(resource: Resource, place: Place, statements: &[String]) -> String {
    let body: String = statements
        .iter()
        .map(|line| format!("    {line}\n"))
        .collect();
    let driver = match place {
        Place::Mono => format!("fn main()\n    println(\"begin\")\n{body}    println(\"end\")\n"),
        Place::GenericFn => format!(
            "fn run<T>(seed T) int\n{body}    return 0\n\n\
             fn main()\n    println(\"begin\")\n    let _done = run(0)\n    println(\"end\")\n"
        ),
    };
    format!(
        "{}\nfn consume(r Res)\n    println(\"consumed\")\n\n{driver}",
        resource.declaration()
    )
}

/// The red cells among `cells`, each with one line of evidence.
pub fn red_resource_cells(cells: &[ResourceCell]) -> Vec<(String, String)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = cells
            .iter()
            .map(|cell| scope.spawn(move || judge(cell).map(|reason| (cell.name.clone(), reason))))
            .collect();
        handles
            .into_iter()
            .filter_map(|handle| handle.join().expect("a resource cell panicked"))
            .collect()
    })
}

/// Why `cell` is red, or `None` when it is green.
fn judge(cell: &ResourceCell) -> Option<String> {
    let (exit_ok, stdout, stderr) = execute(&cell.source);
    let diagnostics = format!("{stdout}{stderr}");
    let clean =
        exit_ok && !stderr.contains("MIRI_LEAK_CHECK:") && !stderr.contains("MIRI_HEAP_GUARD:");
    match &cell.expected {
        Expected::Refused(code) => {
            if diagnostics.contains(code) {
                None
            } else if exit_ok {
                Some(format!("accepted; expected refusal {code}"))
            } else {
                Some(first_line(&diagnostics))
            }
        }
        Expected::Prints(lines) => {
            if !clean {
                return Some(first_line(&diagnostics));
            }
            let printed: Vec<&str> = stdout
                .lines()
                .skip_while(|line| *line != "begin")
                .skip(1)
                .take_while(|line| *line != "end")
                .collect();
            if printed == lines.iter().map(String::as_str).collect::<Vec<_>>() {
                None
            } else {
                Some(format!(
                    "printed `{}`, expected `{}`",
                    printed.join(" | "),
                    lines.join(" | ")
                ))
            }
        }
    }
}

/// The first line of `text` that says something, trimmed for a comment.
fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("failed with no output")
        .chars()
        .take(160)
        .collect()
}

/// Runs `source` the way every matrix program runs.
fn execute(source: &str) -> (bool, String, String) {
    let mut file = NamedTempFile::with_suffix(".mi").expect("create a temporary source file");
    file.write_all(source.as_bytes())
        .expect("write the temporary source file");
    let stdlib = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/stdlib");
    let output = miri_cmd()
        .env("MIRI_LEAK_CHECK", "1")
        .env("MIRI_HEAP_GUARD", "1")
        .env("MIRI_VERIFY_MIR", "1")
        .env("MIRI_STDLIB_PATH", stdlib)
        .env_remove("MIRI_CC")
        .env_remove("CC")
        .timeout(TIMEOUT)
        .arg("run")
        .arg(file.path())
        .output()
        .expect("spawn the miri compiler");
    (
        output.status.success(),
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}
