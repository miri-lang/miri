// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::integration::utils::assert_runs_with_output;

#[test]
fn test_complex_data_structures() {
    assert_runs_with_output(
        r#"
use system.io
use system.collections.map

enum UserStatus
    Active
    Inactive
    Banned(String)

struct User
    name String
    status UserStatus

fn main()
    let u1 = User(name: "Alice", status: UserStatus.Active)
    let u2 = User(name: "Bob", status: UserStatus.Banned("spamming"))
    
    // Test collections of managed types
    var users = [u1, u2] // array of users
    
    var map = {"Alice": u1, "Bob": u2} // map
    
    // Optional retrieval
    let opt_u1 = map.get("Alice")
    let opt_missing = map.get("Charlie")

    // Match on Option
    match opt_u1
        Some(u): println(f"User {u.name} is active")
        None: println("missing")

    match opt_missing
        Some(u): println("found?")
        None: println("Charlie missing as expected")

    // Match on Enum payload
    match u2.status
        UserStatus.Banned(reason): println(f"Bob banned for: {reason}")
        _: println("fine")
"#,
        "User Alice is active\nCharlie missing as expected\nBob banned for: spamming",
    );
}
