// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The conformance matrix: every element type in every slot, under every
//! operation, in every context, checked against an oracle the generator
//! computes itself.
//!
//! Hand-written tests sample this grid one cell at a time, so a defect whose
//! cause is a rule duplicated across layers keeps resurfacing one neighbouring
//! cell at a time. The matrix enumerates the grid instead. Each slot is one
//! test; each of its contexts is judged as one batch program under the leak
//! check and the heap guard, re-run in smaller batches only where a failure
//! cannot be charged to its cell (see `runner`). `KNOWN_RED.toml` holds the
//! cells that are red today, grouped by root cause.
//!
//! Every observed red cell is also written to
//! `target/conformance-matrix/<slot>.toml`, ready to paste into that file, and
//! with `MIRI_MATRIX_KEEP` set every program run is kept beside it under
//! `programs/`, so a red cell can be reproduced by hand.

mod cells;
mod known_red;
mod program;
mod runner;
mod source;
mod types;

use cells::{cells_for, Slot, CONTEXTS, SLOTS};
use runner::{red_cells, Reason};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// Judges every cell of `slot` and fails on any disagreement with
/// `KNOWN_RED.toml`: a red cell it does not list, or a listed cell that is
/// now green.
fn check_slot(slot: Slot) {
    let known = known_red::load().unwrap_or_else(|e| panic!("{e}"));
    let red = observe_slot(slot);
    record_observation(slot, &red);
    let prefix = format!("{}/", slot.token());
    let red_names: BTreeSet<&str> = red.iter().map(|(name, _)| name.as_str()).collect();
    let unlisted: Vec<String> = red
        .iter()
        .filter(|(name, _)| !known.contains_key(name))
        .map(|(name, reason)| format!("    \"{name}\", # {reason}"))
        .collect();
    let healed: Vec<&str> = known
        .keys()
        .filter(|name| name.starts_with(&prefix) && !red_names.contains(name.as_str()))
        .map(String::as_str)
        .collect();
    assert!(
        unlisted.is_empty() && healed.is_empty(),
        "conformance matrix for `{}` disagrees with {}\n\
         red but not listed ({}): add each under its family, or fix it\n{}\n\
         listed but green ({}): remove each, the family fixed it\n{}",
        slot.token(),
        known_red::path().display(),
        unlisted.len(),
        unlisted.join("\n"),
        healed.len(),
        healed.join("\n"),
    );
}

/// The red cells of `slot` across all contexts, judged in parallel.
fn observe_slot(slot: Slot) -> Vec<(String, Reason)> {
    std::thread::scope(|scope| {
        let handles: Vec<_> = CONTEXTS
            .iter()
            .map(|&context| scope.spawn(move || red_cells(&cells_for(slot, context))))
            .collect();
        let mut red: Vec<(String, Reason)> = handles
            .into_iter()
            .flat_map(|handle| handle.join().expect("a matrix context panicked"))
            .collect();
        red.sort();
        red
    })
}

/// Writes the observed red cells where a person updating `KNOWN_RED.toml`
/// can copy them from. Failing to write it never fails the gate.
fn record_observation(slot: Slot, red: &[(String, Reason)]) {
    let directory = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("target/conformance-matrix");
    let lines: String = red
        .iter()
        .map(|(name, reason)| format!("    \"{name}\", # {reason}\n"))
        .collect();
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::write(directory.join(format!("{}.toml", slot.token())), lines);
}

#[test]
fn matrix_known_red_lists_only_real_cells() {
    let known = known_red::load().unwrap_or_else(|e| panic!("{e}"));
    let real: BTreeSet<String> = SLOTS
        .iter()
        .flat_map(|&slot| {
            CONTEXTS
                .iter()
                .flat_map(move |&context| cells_for(slot, context))
        })
        .map(|cell| cell.name)
        .collect();
    let unknown: Vec<&String> = known.keys().filter(|name| !real.contains(*name)).collect();
    assert!(
        unknown.is_empty(),
        "KNOWN_RED.toml names cells the matrix does not generate:\n{unknown:#?}"
    );
}

#[test]
fn matrix_cell_names_are_unique() {
    let mut seen = BTreeSet::new();
    for &slot in SLOTS {
        for &context in CONTEXTS {
            for cell in cells_for(slot, context) {
                assert!(
                    seen.insert(cell.name.clone()),
                    "duplicate cell `{}`",
                    cell.name
                );
            }
        }
    }
}

#[test]
fn matrix_local() {
    check_slot(Slot::Local);
}

#[test]
fn matrix_struct_field() {
    check_slot(Slot::StructField);
}

#[test]
fn matrix_class_field() {
    check_slot(Slot::ClassField);
}

#[test]
fn matrix_inherited_field() {
    check_slot(Slot::InheritedField);
}

#[test]
fn matrix_enum_payload() {
    check_slot(Slot::EnumPayload);
}

#[test]
fn matrix_list_element() {
    check_slot(Slot::ListElement);
}

#[test]
fn matrix_array_element() {
    check_slot(Slot::ArrayElement);
}

#[test]
fn matrix_set_element() {
    check_slot(Slot::SetElement);
}

#[test]
fn matrix_map_key() {
    check_slot(Slot::MapKey);
}

#[test]
fn matrix_map_value() {
    check_slot(Slot::MapValue);
}

#[test]
fn matrix_parameter() {
    check_slot(Slot::Parameter);
}

#[test]
fn matrix_return() {
    check_slot(Slot::Return);
}

#[test]
fn matrix_closure_capture() {
    check_slot(Slot::ClosureCapture);
}

#[test]
fn matrix_generic_field() {
    check_slot(Slot::GenericField);
}
