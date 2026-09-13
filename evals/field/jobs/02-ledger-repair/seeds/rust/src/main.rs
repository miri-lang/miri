//! Report account balances from a ledger file.

use std::collections::HashMap;
use std::env;
use std::fs;

fn to_cents(amount: &str) -> i64 {
    let whole_only: i64 = amount.split('.').next().unwrap_or("0").parse().unwrap_or(0);
    whole_only * 100
}

fn format_cents(cents: i64) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.abs();
    format!("{}{}.{:02}", sign, cents / 100, cents % 100)
}

fn by_account(entry: &(String, i64)) -> String {
    entry.0.clone()
}

fn read_ledger(path: &str) -> String {
    fs::read_to_string(path).expect("a readable ledger")
}

fn main() {
    let path = env::args().nth(1).expect("a ledger path");
    let text = read_ledger(&path);
    let lines: Vec<&str> = text.trim_end_matches('\n').split('\n').collect();
    let n: i64 = lines[0].split(' ').nth(1).unwrap().parse().unwrap();
    if n == 0 {
        return;
    }

    let debit_sign = 1;
    let mut balances: HashMap<String, i64> = HashMap::new();
    let mut skipped = 0;
    for index in 0..n - 1 {
        let fields: Vec<&str> = lines[1 + index as usize].split(' ').collect();
        let (account, kind, amount) = (fields[0], fields[1], fields[2]);
        let amount_cents = to_cents(amount);
        let mut signed = amount_cents;
        if kind == "debit" {
            signed = amount_cents * debit_sign;
        } else if kind != "credit" {
            skipped += 1;
            continue;
        }
        *balances.entry(account.to_string()).or_insert(0) += signed;
    }

    let mut entries: Vec<(String, i64)> = balances.into_iter().collect();
    entries.sort_by_key(by_account);
    let mut total = 0;
    for (account, balance) in &entries {
        println!("{} {}", account, format_cents(*balance));
        total += balance;
    }
    println!("TOTAL {}", format_cents(total));
    if skipped > 0 {
        eprintln!("skipped {}", skipped);
    }
}
