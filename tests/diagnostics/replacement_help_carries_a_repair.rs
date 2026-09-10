// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A help sentence that names the exact token to write instead has already done
//! the thinking, and a consumer that has to re-type the token pays for thinking
//! the compiler did and threw away.
//!
//! Two gates. The first drives a program for every such sentence and requires
//! the diagnostic to carry the edit as well as the words. The second counts the
//! sentences the compiler can build, so a new one cannot be added without
//! either a repair or a written reason there is none — which is what keeps this
//! from being closed one instance at a time.

use crate::utils::miri_cmd;
use miri::diagnostics::json::DiagnosticsEnvelope;
use std::fs;
use std::path::{Path, PathBuf};

/// A program whose diagnostic names the token to write instead.
struct Naming {
    what: &'static str,
    code: &'static str,
    source: &'static str,
}

/// One program per shape of help that names a replacement.
///
/// The two families are a near miss on a name the program could have written,
/// and a binding nothing reads. Every member of both is here: a shape missing
/// from this table is caught by the inventory below rather than passing
/// unnoticed.
const NAMINGS: &[Naming] = &[
    Naming {
        what: "a near-miss method on a collection",
        code: "MER_TYP_033",
        source: "fn main()\n    let accounts = [1, 2, 3]\n    println(f\"{accounts.len()}\")\n",
    },
    Naming {
        what: "a near-miss field on a struct",
        code: "MER_TYP_033",
        source: "struct Point\n    width int\n    height int\n\nfn main()\n    let p = \
                 Point(1, 2)\n    println(f\"{p.heigth}\")\n",
    },
    Naming {
        what: "a near-miss method on a trait",
        code: "MER_TYP_033",
        source: "trait Shape\n    fn area() int\n\nfn describe(s Shape) int\n    return \
                 s.arae()\n\nfn main()\n    println(\"x\")\n",
    },
    Naming {
        what: "a near-miss variant on an enum",
        code: "MER_TYP_038",
        source: "enum Color\n    Red\n    Green\n\nfn main()\n    let c = Color.Rd\n    \
                 println(f\"{c}\")\n",
    },
    Naming {
        what: "a near-miss type name",
        code: "MER_TYP_043",
        source: "fn f(x Strng) int\n    1\n\nfn main()\n    println(f\"{f(1)}\")\n",
    },
    Naming {
        what: "a near-miss variable name",
        code: "MER_TYP_034",
        source: "fn main()\n    let count = 1\n    println(f\"{cout}\")\n",
    },
    Naming {
        what: "a local nothing reads",
        code: "MER_TYP_071",
        source: "fn main()\n    let total = 42\n",
    },
    Naming {
        what: "a parameter its body never reads",
        code: "MER_TYP_072",
        source: "fn greet(name String)\n    println(\"hi\")\n\nfn main()\n    greet(\"a\")\n",
    },
];

/// Whether `help` names the exact token to write instead.
///
/// Both families read the same way to a consumer: the sentence contains the
/// replacement, spelled, so nothing is left to work out.
fn names_a_replacement(help: &str) -> bool {
    help.contains("Did you mean '") || help.contains("name it '_")
}

/// The envelope `miri fix` reports for `source`, written under `name`.
fn plan_for(name: &str, source: &str) -> (PathBuf, DiagnosticsEnvelope) {
    let directory = std::env::temp_dir().join(format!("miri-naming-{}", name));
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(&directory).expect("could not create the fixture directory");
    let file = directory.join("main.mi");
    fs::write(&file, source).expect("could not write the fixture source");

    let output = miri_cmd()
        .arg("fix")
        .arg("--format")
        .arg("json")
        .arg(&file)
        .output()
        .expect("failed to run the fix command");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let envelope: DiagnosticsEnvelope =
        serde_json::from_str(&stdout).expect("fix did not emit a parseable envelope");
    (file, envelope)
}

#[test]
fn test_every_help_that_names_a_replacement_carries_the_edit() {
    for (index, naming) in NAMINGS.iter().enumerate() {
        let (file, envelope) = plan_for(&format!("carries-{}", index), naming.source);
        let naming_diagnostics: Vec<_> = envelope
            .diagnostics
            .iter()
            .filter(|diagnostic| diagnostic.help.as_deref().is_some_and(names_a_replacement))
            .collect();

        assert!(
            !naming_diagnostics.is_empty(),
            "{} should raise a help that names the replacement; the run reported {:?}",
            naming.what,
            envelope.diagnostics
        );
        for diagnostic in &naming_diagnostics {
            assert_eq!(
                diagnostic.code.as_deref(),
                Some(naming.code),
                "{} should arrive on {}",
                naming.what,
                naming.code
            );
            assert!(
                diagnostic.repair.is_some(),
                "{} names a replacement in prose and ships no repair: {:?}",
                naming.what,
                diagnostic
            );
        }
        let _ = fs::remove_dir_all(file.parent().expect("the fixture has a directory"));
    }
}

#[test]
fn test_applying_the_edit_answers_the_help_that_named_it() {
    for (index, naming) in NAMINGS.iter().enumerate() {
        let (file, _) = plan_for(&format!("answers-{}", index), naming.source);

        let applied = miri_cmd()
            .arg("fix")
            .arg("--apply")
            .arg("--yes")
            .arg("--allow-risky")
            .arg(&file)
            .output()
            .expect("failed to run the fix command");
        assert!(
            applied.status.success(),
            "applying the repair for {} should succeed: {}",
            naming.what,
            String::from_utf8_lossy(&applied.stderr)
        );

        let (_, after) = plan_for(
            &format!("answers-{}", index),
            &fs::read_to_string(&file).expect("could not read the repaired source"),
        );
        assert!(
            !after
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.code.as_deref() == Some(naming.code)),
            "{} still raises {} after its own repair was applied",
            naming.what,
            naming.code
        );
        let _ = fs::remove_dir_all(file.parent().expect("the fixture has a directory"));
    }
}

/// A help sentence the compiler builds that names the token to write instead.
///
/// `sites` is how many times the compiler source spells it. The count is part
/// of the gate: a new place that says the same thing changes it, so the sentence
/// cannot be added without this file deciding what happens to the edit.
struct Sentence {
    shape: &'static str,
    sites: usize,
    /// Why no repair travels with it, or empty when one does.
    unrepaired_because: &'static str,
}

/// Every replacement-naming sentence the compiler can build.
const SENTENCES: &[Sentence] = &[
    Sentence {
        shape: "Did you mean '{}'?",
        sites: 6,
        unrepaired_because: "",
    },
    Sentence {
        shape: "name it '_{}'",
        sites: 2,
        unrepaired_because: "",
    },
    Sentence {
        shape: "Did you mean 'self.{}()'?",
        sites: 1,
        unrepaired_because: "the edit inserts a receiver and a call, and how the call is \
                             spelled depends on arguments the reader has not written",
    },
    Sentence {
        shape: "Did you mean 'self.{}'?",
        sites: 1,
        unrepaired_because: "the name is reported where it is read, and the reader may have \
                             meant to declare the local rather than reach the field",
    },
    Sentence {
        shape: "did you mean '{}'?",
        sites: 1,
        unrepaired_because: "the near miss is in a command-line argument, so there is no \
                             source text an edit could rewrite",
    },
];

/// The opening of a sentence that goes on to name a replacement.
///
/// Counting these as well as the shapes is what makes the inventory a gate
/// rather than a list: a sentence spelled a new way is still one of these, so it
/// raises the total and no shape accounts for it.
const NAMING_OPENINGS: &[&str] = &["Did you mean '", "did you mean '", "name it '_"];

/// Every `.rs` file under `src/`, minus its comment lines.
///
/// A doc comment quoting a message is describing one, not building one, and a
/// gate that counted both would be answered by editing prose.
fn compiler_sources() -> String {
    fn walk(directory: &Path, buffer: &mut String) -> std::io::Result<()> {
        for entry in fs::read_dir(directory)? {
            let path = entry?.path();
            if path.is_dir() {
                walk(&path, buffer)?;
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                for line in fs::read_to_string(&path)?.lines() {
                    if line.trim_start().starts_with("//") {
                        continue;
                    }
                    buffer.push_str(line);
                    buffer.push('\n');
                }
            }
        }
        Ok(())
    }

    let mut buffer = String::new();
    walk(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("src"),
        &mut buffer,
    )
    .expect("could not read the compiler sources");
    buffer
}

#[test]
fn test_no_replacement_naming_sentence_is_added_without_deciding_about_its_edit() {
    let sources = compiler_sources();
    for sentence in SENTENCES {
        let sites = sources.matches(sentence.shape).count();
        assert_eq!(
            sites, sentence.sites,
            "`{}` is written {} times in src/ and this gate accounts for {}; a new place \
             that names a replacement needs a repair, or a reason here that it has none",
            sentence.shape, sites, sentence.sites
        );
    }

    let accounted: usize = SENTENCES.iter().map(|sentence| sentence.sites).sum();
    let written: usize = NAMING_OPENINGS
        .iter()
        .map(|opening| sources.matches(opening).count())
        .sum();
    assert!(accounted > 0, "the gate inspected nothing");
    assert_eq!(
        written, accounted,
        "src/ opens {} sentences that name a replacement and this gate accounts for {}; \
         a sentence spelled a new way needs its own entry above, carrying a repair or a \
         written reason it has none",
        written, accounted
    );
}

#[test]
fn test_a_sentence_excused_from_carrying_an_edit_says_why() {
    for sentence in SENTENCES {
        if sentence.unrepaired_because.is_empty() {
            continue;
        }
        assert!(
            sentence.unrepaired_because.len() > 30,
            "`{}` is excused without a real reason",
            sentence.shape
        );
    }
}
