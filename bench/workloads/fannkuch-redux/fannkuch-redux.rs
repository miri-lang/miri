// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko
// port-checklist: reviewed 2026-10-16 by review-agent sha256=495e1af43dbfafc39281fef3163f0dcb741b869a61860bb89d1ebda7610a5f89

// fannkuch-redux: pancake flips over every permutation (see fannkuch-redux.mi).

use std::time::Instant;

fn count_flips(perm: &[usize], p: &mut [usize]) -> i64 {
    p.copy_from_slice(perm);
    let mut flips = 0;
    let mut first = p[0];
    while first != 0 {
        p[..=first].reverse();
        flips += 1;
        first = p[0];
    }
    flips
}

fn kernel(n: i64) -> String {
    let n = n as usize;
    let mut perm: Vec<usize> = (0..n).collect();
    let mut count = vec![0; n];
    let mut scratch = vec![0; n];
    let mut checksum: i64 = 0;
    let mut max_flips = 0;
    let mut index: i64 = 0;
    loop {
        let flips = count_flips(&perm, &mut scratch);
        max_flips = max_flips.max(flips);
        checksum += if index % 2 == 0 { flips } else { -flips };
        index += 1;
        let mut r = 1;
        loop {
            if r >= n {
                return format!("{checksum} {max_flips}");
            }
            perm[..=r].rotate_left(1);
            count[r] += 1;
            if count[r] <= r {
                break;
            }
            count[r] = 0;
            r += 1;
        }
    }
}

fn run_hot(n: i64) -> String {
    let expected = kernel(n);
    let mut agree = true;
    for _ in 1..5 {
        let checksum = kernel(n);
        agree = agree && checksum == expected;
    }
    for _ in 0..5 {
        let start = Instant::now();
        let checksum = kernel(n);
        let elapsed = start.elapsed().as_nanos();
        eprintln!("hot_ns={elapsed}");
        agree = agree && checksum == expected;
    }
    if !agree {
        eprintln!("checksum differs between iterations");
        std::process::exit(1);
    }
    expected
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let text = std::fs::read_to_string(&args[1]).expect("readable input");
    let n: i64 = text.trim().parse().expect("an integer parameter");
    let hot = args.get(2).is_some_and(|a| a == "hot");
    println!("{}", if hot { run_hot(n) } else { kernel(n) });
}
