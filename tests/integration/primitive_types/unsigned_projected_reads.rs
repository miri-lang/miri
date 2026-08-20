// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Unsigned integers read out of a field or a collection element.
//!
//! Such a read reaches its value through a projection, so the type that decides
//! how the value widens is the projected one, not the type of the local the read
//! starts from. Judging by the base local reports a class or a list — not an
//! integer at all — and the value then widens as signed, turning every unsigned
//! value with its top bit set negative.

use crate::integration::utils::*;

#[test]
fn test_u8_field_above_signed_range_reads_back_whole() {
    assert_runs_with_output(
        r#"
class Counter
    var count u8
    public fn init(start u8)
        self.count = start

fn main()
    let c = Counter(200)
    println(f"{c.count}")
"#,
        "200",
    );
}

#[test]
fn test_u8_field_compares_as_unsigned() {
    assert_runs_with_output(
        r#"
class Counter
    var count u8
    public fn init(start u8)
        self.count = start

fn main()
    let c = Counter(200)
    if c.count > 100
        println("above")
    else
        println("below")
"#,
        "above",
    );
}

#[test]
fn test_u16_field_above_signed_range_reads_back_whole() {
    assert_runs_with_output(
        r#"
class Reading
    var value u16
    public fn init(v u16)
        self.value = v

fn main()
    let r = Reading(60000)
    println(f"{r.value}")
"#,
        "60000",
    );
}

#[test]
fn test_u8_list_element_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<u8>([])
    l.push(200)
    println(f"{l[0]}")
"#,
        "200",
    );
}

#[test]
#[ignore = "Sub-word list elements past index 0 read back as zero: the element \
stride the list is built at does not match the stride its reads address it by. \
The source array literal inside List<u8>([...]) is typed as a bare Array with no \
element type, so it is built at the pointer width while every read of the list \
steps by one byte, and each element after the first lands between slots. Affects \
every element narrower than a word (u8/i8/u16/i16/i32/u32/f32), not just \
unsigned ones. Index 0 is correct because both strides start there."]
fn test_u8_list_literal_element_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List<u8>([200, 7])
    println(f"{l[0]}")
    println(f"{l[1]}")
"#,
        "200
7",
    );
}

#[test]
fn test_u16_list_element_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<u16>([])
    l.push(60000)
    println(f"{l[0]}")
"#,
        "60000",
    );
}

#[test]
fn test_signed_narrow_field_keeps_its_sign() {
    assert_runs_with_output(
        r#"
class Delta
    var value i8
    public fn init(v i8)
        self.value = v

fn main()
    let d = Delta(-56)
    println(f"{d.value}")
"#,
        "-56",
    );
}

#[test]
fn test_signed_narrow_list_element_keeps_its_sign() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<i8>([])
    l.push(-56)
    println(f"{l[0]}")
"#,
        "-56",
    );
}

#[test]
fn test_u8_set_membership_above_signed_range() {
    assert_runs_with_output(
        r#"
use system.collections.set

fn main()
    var s = Set<u8>({})
    s.add(200)
    println(f"{s.contains(200)}")
"#,
        "true",
    );
}

#[test]
#[ignore = "Sub-word list elements past index 0 read back as zero — see \
test_u8_list_literal_element_reads_back_whole for the stride mismatch. Recorded \
at i32 as well to show the defect is about element width, not signedness."]
fn test_i32_list_second_element_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    let l = List<i32>([200, 7])
    println(f"{l[0]}")
    println(f"{l[1]}")
"#,
        "200
7",
    );
}

#[test]
#[ignore = "Sub-word list elements past index 0 read back as zero — see \
test_u8_list_literal_element_reads_back_whole. Pushing rather than building from \
a literal reaches the same mismatch."]
fn test_u8_list_second_pushed_element_reads_back_whole() {
    assert_runs_with_output(
        r#"
use system.collections.list

fn main()
    var l = List<u8>([])
    l.push(200)
    l.push(7)
    println(f"{l[0]}")
    println(f"{l[1]}")
"#,
        "200
7",
    );
}
