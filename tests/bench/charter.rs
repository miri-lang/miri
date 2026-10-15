// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The benchmark charter and its machine-readable twin must say the same thing.
//!
//! `bench/CHARTER.md` is what a reader trusts; `bench/charter.toml` is what the
//! runner executes. A threshold, build line or tier edited in one and not the
//! other would publish numbers produced under rules nobody wrote down, so every
//! value, command and tier is compared here, and every key the prose cites must
//! exist.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

const NOT_APPLICABLE: &str = "n/a";

/// Top-level tables that hold lists and commands rather than parameters; the
/// tier and build-line tests cover them.
const NON_PARAMETER_TABLES: [&str; 2] = ["tiers", "languages"];

fn bench_file(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("bench")
        .join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

fn charter_toml() -> toml::Table {
    bench_file("charter.toml")
        .parse()
        .unwrap_or_else(|e| panic!("bench/charter.toml does not parse: {e}"))
}

fn charter_md() -> String {
    bench_file("CHARTER.md")
}

/// The data rows of a Markdown table, each row's cells verbatim.
struct Table {
    rows: Vec<Vec<String>>,
}

fn split_row(line: &str) -> Vec<String> {
    let inner = line
        .strip_prefix('|')
        .and_then(|rest| rest.strip_suffix('|'))
        .unwrap_or_else(|| panic!("table row does not start and end with `|`: {line}"));
    inner
        .replace("\\|", "\u{0}")
        .split('|')
        .map(|cell| cell.trim().replace('\u{0}', "|"))
        .collect()
}

/// The single table in `text`, checked for its header and for rows of the
/// header's width.
fn table(text: &str, expected_header: &[&str]) -> Table {
    let mut lines = text.lines().filter(|line| line.starts_with('|'));
    let header = split_row(lines.next().expect("section holds a table"));
    assert_eq!(header, expected_header, "unexpected table header");
    let separator = lines.next().expect("table has a separator row");
    assert!(
        separator.starts_with("|---"),
        "table separator missing: {separator}"
    );
    let rows = lines
        .map(|line| {
            let cells = split_row(line);
            assert_eq!(cells.len(), header.len(), "row has the wrong width: {line}");
            cells
        })
        .collect();
    Table { rows }
}

/// The contents of a cell that is one code span.
fn code(cell: &str) -> &str {
    cell.strip_prefix('`')
        .and_then(|rest| rest.strip_suffix('`'))
        .unwrap_or(cell)
}

/// Every code span in `text`.
fn code_spans(text: &str) -> Vec<&str> {
    text.split('`').skip(1).step_by(2).collect()
}

/// The text of the `## <heading>` section, up to the next `## ` heading.
fn section<'a>(md: &'a str, heading: &str) -> &'a str {
    let marker = format!("\n## {heading}\n");
    let start = md
        .find(&marker)
        .unwrap_or_else(|| panic!("CHARTER.md has no `## {heading}` section"))
        + marker.len();
    let rest = &md[start..];
    rest.find("\n## ").map_or(rest, |end| &rest[..end])
}

/// Inserts, refusing a key that is already present.
fn insert_unique<V>(map: &mut BTreeMap<String, V>, key: String, value: V, what: &str) {
    assert!(
        !map.contains_key(&key),
        "{what} `{key}` is listed more than once"
    );
    map.insert(key, value);
}

/// Every parameter leaf of the table, keyed by its dotted path and spelled as
/// TOML spells it, so `3` and `3.0` are different values.
fn parameter_leaves(table: &toml::Table, prefix: &str, out: &mut BTreeMap<String, String>) {
    for (key, value) in table {
        if prefix.is_empty() && NON_PARAMETER_TABLES.contains(&key.as_str()) {
            continue;
        }
        let path = if prefix.is_empty() {
            key.clone()
        } else {
            format!("{prefix}.{key}")
        };
        match value {
            toml::Value::Integer(n) => insert_unique(out, path, n.to_string(), "parameter"),
            toml::Value::Float(x) => insert_unique(out, path, format!("{x:?}"), "parameter"),
            toml::Value::Table(inner) => parameter_leaves(inner, &path, out),
            toml::Value::String(_)
            | toml::Value::Boolean(_)
            | toml::Value::Datetime(_)
            | toml::Value::Array(_) => {
                panic!("`{path}` is not a number; parameters outside [tiers] and [languages] are numeric")
            }
        }
    }
}

fn documented_parameters(md: &str) -> BTreeMap<String, String> {
    let parameters = table(section(md, "Parameters"), &["Key", "Value", "Meaning"]);
    let mut out = BTreeMap::new();
    for row in parameters.rows {
        insert_unique(
            &mut out,
            code(&row[0]).to_string(),
            code(&row[1]).to_string(),
            "parameter",
        );
    }
    out
}

#[test]
fn every_parameter_is_documented_with_the_same_value() {
    let mut in_toml = BTreeMap::new();
    parameter_leaves(&charter_toml(), "", &mut in_toml);
    let in_md = documented_parameters(&charter_md());
    assert!(!in_toml.is_empty(), "charter.toml carries no parameters");
    assert_eq!(
        in_md, in_toml,
        "the Parameters table in bench/CHARTER.md and bench/charter.toml disagree"
    );
}

fn resolves(toml: &toml::Table, dotted: &str) -> bool {
    let mut segments = dotted.split('.');
    let Some(first) = segments.next() else {
        return false;
    };
    let mut value = toml.get(first);
    for segment in segments {
        value = value
            .and_then(|v| v.as_table())
            .and_then(|t| t.get(segment));
    }
    value.is_some()
}

#[test]
fn every_key_the_charter_cites_exists() {
    let toml = charter_toml();
    let md = charter_md();
    let cited: BTreeSet<&str> = code_spans(&md)
        .into_iter()
        .filter(|span| {
            span.split('.').count() > 1
                && span.split('.').all(|segment| {
                    !segment.is_empty()
                        && segment.chars().all(|c| c.is_ascii_lowercase() || c == '_')
                })
                && toml.contains_key(span.split('.').next().unwrap_or_default())
        })
        .collect();
    assert!(!cited.is_empty(), "the charter cites no parameter by key");
    let missing: Vec<&&str> = cited.iter().filter(|key| !resolves(&toml, key)).collect();
    assert!(
        missing.is_empty(),
        "CHARTER.md cites keys charter.toml lacks: {missing:?}"
    );
}

#[test]
fn measured_run_counts_are_odd() {
    let toml = charter_toml();
    let measured = toml
        .get("runs")
        .and_then(|runs| runs.get("measured"))
        .and_then(toml::Value::as_table)
        .expect("charter.toml has [runs.measured]");
    for (size, count) in measured {
        let count = count.as_integer().expect("a run count is an integer");
        assert_eq!(
            count % 2,
            1,
            "runs.measured.{size} is {count}; the median must be an observed run"
        );
    }
}

fn language_table(toml: &toml::Table) -> &toml::Table {
    toml.get("languages")
        .and_then(toml::Value::as_table)
        .expect("charter.toml has a [languages] table")
}

fn command(language: &toml::Table, key: &str) -> String {
    language
        .get(key)
        .and_then(toml::Value::as_str)
        .unwrap_or(NOT_APPLICABLE)
        .to_string()
}

#[test]
fn every_build_and_run_line_is_documented_verbatim() {
    let toml = charter_toml();
    let md = charter_md();
    let lines = table(
        section(&md, "4. Build and run lines"),
        &["Language", "Build", "Native build", "Run"],
    );
    let mut rows = BTreeMap::new();
    for row in lines.rows {
        let commands: Vec<String> = row[1..].iter().map(|cell| code(cell).to_string()).collect();
        insert_unique(&mut rows, code(&row[0]).to_string(), commands, "language");
    }
    let languages = language_table(&toml);
    let documented: Vec<&String> = rows.keys().collect();
    let declared: Vec<&String> = languages.keys().collect();
    assert_eq!(
        documented, declared,
        "the build table lists different languages"
    );
    for (id, value) in languages {
        let language = value.as_table().expect("each language is a table");
        let expected = ["build", "native_build", "run"].map(|key| command(language, key));
        assert_eq!(
            rows[id], expected,
            "build/native/run lines for `{id}` disagree"
        );
    }
}

fn toml_tiers(toml: &toml::Table) -> BTreeMap<String, BTreeSet<String>> {
    toml.get("tiers")
        .and_then(toml::Value::as_table)
        .expect("charter.toml has a [tiers] table")
        .iter()
        .map(|(tier, members)| {
            let ids = members
                .as_array()
                .expect("a tier is an array")
                .iter()
                .map(|member| {
                    member
                        .as_str()
                        .expect("a tier member is a string")
                        .to_string()
                })
                .collect();
            (tier.clone(), ids)
        })
        .collect()
}

#[test]
fn the_documented_tiers_match_the_declared_tiers() {
    let md = charter_md();
    let tiers = table(
        section(&md, "2. Languages and tiers"),
        &["Tier", "Languages", "Role"],
    );
    let mut documented = BTreeMap::new();
    for row in &tiers.rows {
        let ids = code_spans(&row[1])
            .into_iter()
            .map(str::to_string)
            .collect();
        insert_unique(&mut documented, row[0].to_lowercase(), ids, "tier");
    }
    assert_eq!(
        documented,
        toml_tiers(&charter_toml()),
        "the tier table and [tiers] disagree"
    );
}

#[test]
fn every_language_sits_in_exactly_one_tier_and_names_its_comment() {
    let toml = charter_toml();
    let mut tier_of = BTreeMap::new();
    for (tier, members) in toml_tiers(&toml) {
        for id in members {
            insert_unique(&mut tier_of, id, tier.clone(), "tiered language");
        }
    }
    let languages = language_table(&toml);
    let declared: Vec<&String> = languages.keys().collect();
    let tiered: Vec<&String> = tier_of.keys().collect();
    assert_eq!(
        declared, tiered,
        "a language is declared without a tier, or tiered without being declared"
    );
    for (id, value) in languages {
        let comment = value.get("checklist_comment").and_then(toml::Value::as_str);
        assert!(
            matches!(comment, Some("//") | Some("#")),
            "`{id}` must name the line comment its port-checklist line uses, got {comment:?}"
        );
    }
}

#[test]
fn the_charter_version_matches_its_latest_changelog_entry() {
    let md = charter_md();
    let version = charter_toml()
        .get("charter_version")
        .and_then(toml::Value::as_integer)
        .expect("charter.toml has an integer charter_version");
    assert!(
        md.contains(&format!("**Charter version: {version}.**")),
        "CHARTER.md does not state charter version {version}"
    );
    let latest = section(&md, "Changelog")
        .lines()
        .filter_map(|line| line.strip_prefix("- **Version "))
        .filter_map(|rest| rest.split(',').next()?.parse::<i64>().ok())
        .max();
    assert_eq!(
        latest,
        Some(version),
        "the newest changelog entry must be for the current charter version"
    );
}
