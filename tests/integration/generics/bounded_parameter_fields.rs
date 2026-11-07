// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! A field read or written through a type parameter bounded by a class
//! (`T extends Animal`) reaches the field of the class the body is compiled
//! for: the bound class itself, or a subclass that inherits the field. A field
//! the bound does not declare is refused where it is written.

use super::utils::*;

const ANIMALS: &str = r#"
use system.io

trait Named
    fn title() String

class Animal
    var legs int
    var sound String

class Dog extends Animal implements Named
    var name String
    fn title() String
        return self.name

class Puppy extends Dog
    var age int

fn legs_of<T extends Animal>(x T) int
    return x.legs

fn sound_of<T extends Animal>(x T) String
    return x.sound
"#;

fn with(main: &str) -> String {
    format!("{ANIMALS}\n{main}")
}

#[test]
fn a_primitive_field_is_read_through_a_class_bound_at_the_base_class() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    let a = Animal(legs: 4, sound: "moo")
    println(f"{legs_of(a)}")
"#,
        ),
        "4",
    );
}

#[test]
fn a_primitive_field_is_read_through_a_class_bound_at_a_subclass() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    let d = Dog(legs: 3, sound: "woof", name: "rex")
    println(f"{legs_of(d)}")
"#,
        ),
        "3",
    );
}

#[test]
fn a_managed_field_is_read_through_a_class_bound_at_the_base_class() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    let a = Animal(legs: 4, sound: "m" + "oo")
    println(sound_of(a))
"#,
        ),
        "moo",
    );
}

#[test]
fn a_managed_field_is_read_through_a_class_bound_at_a_subclass() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    let d = Dog(legs: 3, sound: "wo" + "of", name: "rex")
    println(sound_of(d))
    println(d.sound)
"#,
        ),
        "woof\nwoof",
    );
}

#[test]
fn a_field_inherited_two_levels_up_is_read_through_a_class_bound() {
    assert_heap_guard_output(
        &with(
            r#"
fn main()
    let p = Puppy(legs: 2, sound: "y" + "ip", name: "bit", age: 1)
    println(f"{legs_of(p)} {sound_of(p)}")
"#,
        ),
        "2 yip",
    );
}

#[test]
fn a_field_is_written_through_a_class_bound_on_an_out_parameter() {
    assert_heap_guard_output(
        &with(
            r#"
fn grow<T extends Animal>(x out T)
    x.legs = 5
    x.sound = "gr" + "r"

fn main()
    var d = Dog(legs: 3, sound: "woof", name: "rex")
    grow(d)
    println(f"{d.legs} {d.sound}")
"#,
        ),
        "5 grr",
    );
}

#[test]
fn a_field_is_read_through_a_class_and_trait_bound_list() {
    assert_heap_guard_output(
        &with(
            r#"
fn describe<T extends Animal and Named>(x T) String
    return f"{x.title()} has {x.legs} legs and says {x.sound}"

fn main()
    println(describe(Dog(legs: 4, sound: "woof", name: "rex")))
"#,
        ),
        "rex has 4 legs and says woof",
    );
}

#[test]
fn a_generic_class_method_reads_a_field_of_its_class_bounded_item() {
    assert_heap_guard_output(
        &with(
            r#"
class Box<T extends Animal>
    var item T
    fn legs() int
        return self.item.legs
    fn sound() String
        return self.item.sound

fn main()
    let b = Box<Dog>(item: Dog(legs: 4, sound: "wo" + "of", name: "rex"))
    println(f"{b.legs()} {b.sound()}")
    let c = Box<Animal>(item: Animal(legs: 2, sound: "ch" + "irp"))
    println(f"{c.legs()} {c.sound()}")
"#,
        ),
        "4 woof\n2 chirp",
    );
}

#[test]
fn a_field_the_class_bound_does_not_declare_is_refused() {
    assert_compiler_error(
        &with(
            r#"
fn name_of<T extends Animal>(x T) String
    return x.name

fn main()
    println(name_of(Dog(legs: 4, sound: "woof", name: "rex")))
"#,
        ),
        "Type 'Animal' has no field or method 'name'",
    );
}
