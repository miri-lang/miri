// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! What emptying a place of a method's receiver takes out of it.

use super::*;
use crate::ast::factory::{identifier, index, member};

fn receiver() -> Expression {
    identifier(RECEIVER_PARAM_NAME)
}

fn params(names: &[&str]) -> HashSet<String> {
    names.iter().map(|name| name.to_string()).collect()
}

#[test]
fn emptying_the_receiver_takes_the_whole_receiver_out() {
    assert_eq!(
        emptied_at(&receiver(), &params(&[])),
        Some(Emptied::Receiver)
    );
}

#[test]
fn emptying_a_field_of_the_receiver_takes_that_field_out() {
    let place = member(receiver(), identifier("slot"));
    assert_eq!(
        emptied_at(&place, &params(&[])),
        Some(Emptied::Field {
            name: "slot".to_string()
        })
    );
}

#[test]
fn emptying_an_element_of_the_receiver_at_a_parameter_takes_that_element_out() {
    let place = index(member(receiver(), identifier("cells")), identifier("i"));
    assert_eq!(
        emptied_at(&place, &params(&["i"])),
        Some(Emptied::Element {
            index: "i".to_string()
        })
    );
}

#[test]
fn emptying_a_field_of_another_binding_takes_nothing_out_of_the_receiver() {
    let place = member(identifier("other"), identifier("slot"));
    assert_eq!(emptied_at(&place, &params(&[])), None);
}

#[test]
fn emptying_a_field_of_a_field_of_the_receiver_is_not_recorded() {
    let place = member(member(receiver(), identifier("inner")), identifier("slot"));
    assert_eq!(emptied_at(&place, &params(&[])), None);
}
