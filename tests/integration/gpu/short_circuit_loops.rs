// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
//
// A kernel loop whose condition short-circuits. The header tests only the
// first operand, so the loop can end on either one; whichever ends it, the
// statements after the loop must run.

use super::device::require_gpu_int64;
use super::utils::*;

/// One loop condition over the counter `k` and the thread index `idx`, and the
/// same condition as a Rust predicate, the oracle for every thread.
struct Condition {
    miri: &'static str,
    holds: fn(i64, i64) -> bool,
}

const CONDITIONS: [Condition; 5] = [
    Condition {
        miri: "k < 3",
        holds: |k, _| k < 3,
    },
    Condition {
        miri: "k < 3 and idx < 2",
        holds: |k, idx| k < 3 && idx < 2,
    },
    Condition {
        miri: "k < idx and k < 2",
        holds: |k, idx| k < idx && k < 2,
    },
    Condition {
        miri: "not (k >= 3) and idx != 1",
        holds: |k, idx| k < 3 && idx != 1,
    },
    Condition {
        miri: "k < 3 and (idx < 2 or idx == 3)",
        holds: |k, idx| k < 3 && (idx < 2 || idx == 3),
    },
];

/// What the program prints: for each thread, the counter the loop stopped at
/// plus a hundred, written by the statement after the loop.
fn expected(holds: fn(i64, i64) -> bool) -> String {
    (0..4)
        .map(|idx| {
            let mut k = 0;
            while holds(k, idx) {
                k += 1;
            }
            (k + 100).to_string()
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Every condition, each thread ending its loop on whichever operand fails
/// first, runs the store after the loop with the counter the loop reached.
#[test]
#[cfg_attr(
    not(feature = "gpu_hardware"),
    ignore = "requires a real GPU; runs on the macos-14 hardware job"
)]
fn every_short_circuit_loop_condition_reaches_the_statements_after_the_loop() {
    require_gpu_int64();
    for condition in CONDITIONS {
        let code = format!(
            "
use system.io

fn main()
    gpu var out = [0, 0, 0, 0]
    forall idx in 0..4
        var k = 0
        while {}
            k = k + 1
        out[idx] = k + 100
    let host = out
    println(f\"{{host[0]}} {{host[1]}} {{host[2]}} {{host[3]}}\")
",
            condition.miri
        );
        assert_runs_with_output(&code, &expected(condition.holds));
    }
}
