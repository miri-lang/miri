// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri::mir::{Local, Place, PlaceElem};

fn place(local: usize, projection: Vec<PlaceElem>) -> Place {
    Place {
        local: Local(local),
        projection,
    }
}

fn index(local: usize) -> PlaceElem {
    PlaceElem::Index(Local(local))
}

#[test]
fn test_index_locals_lists_every_index_outermost_first() {
    let nested = place(1, vec![index(2), PlaceElem::Field(0), index(3)]);
    assert_eq!(
        nested.index_locals().collect::<Vec<_>>(),
        vec![Local(2), Local(3)]
    );
    assert_eq!(
        place(1, vec![PlaceElem::Field(1)]).index_locals().count(),
        0
    );
}

#[test]
fn test_a_place_covers_itself_and_everything_inside_it() {
    let element = place(1, vec![index(2)]);
    assert!(element.covers(&element));
    assert!(element.covers(&place(1, vec![index(2), PlaceElem::Field(0)])));
    assert!(place(1, Vec::new()).covers(&element));
    assert!(!element.covers(&place(1, Vec::new())));
    assert!(!element.covers(&place(4, vec![index(2)])));
}

#[test]
fn test_any_index_covers_any_other_index() {
    assert!(place(1, vec![index(2)]).covers(&place(1, vec![index(3)])));
}

#[test]
fn test_a_field_covers_only_the_same_field_and_never_an_index() {
    let first = place(1, vec![PlaceElem::Field(0)]);
    assert!(!first.covers(&place(1, vec![PlaceElem::Field(1)])));
    assert!(!first.covers(&place(1, vec![index(2)])));
    assert!(!place(1, vec![index(2)]).covers(&first));
}

#[test]
fn test_a_dereference_covers_and_is_covered_by_anything() {
    let through = place(1, vec![PlaceElem::Deref]);
    assert!(through.covers(&place(1, vec![PlaceElem::Field(3)])));
    assert!(place(1, vec![index(2)]).covers(&through));
}
