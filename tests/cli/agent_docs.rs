// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The website's "Miri for agents" guide is generated from this repository,
//! and these tests keep it that way.
//!
//! Every command the guide shows is listed in `examples/agents/runs.toml`, and
//! the first test replays each one against the compiler under test, so a
//! command the guide shows cannot stop working without failing the suite.
//!
//! The last test compares the website's vendored copies with a fresh render:
//! the skill packs and example programs as exact strings, the code registry
//! and the JSON schema as parsed JSON, and each recorded command's output. It reads
//! the sibling `../miri-lang.org` checkout and skips with a log line when that
//! checkout is absent, so a green suite on a machine without it proves nothing
//! about the website. After editing a skill pack, an example, the schema or any
//! output the guide records, regenerate the website copy:
//!
//! ```text
//! cd ../miri-lang.org && python3 tools/gen_agent_docs.py
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One command a session runs, with the exit status the guide promises.
struct Step {
    args: Vec<String>,
    exit: i32,
}

/// A directory's worth of commands, run in order over copies of `files`.
struct Session {
    id: String,
    files: Vec<String>,
    steps: Vec<Step>,
}

/// What one step printed, with the session directory and timings normalised.
#[derive(Debug, PartialEq)]
struct Recorded {
    args: Vec<String>,
    exit: i32,
    stdout: String,
    stderr: String,
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn examples_dir() -> PathBuf {
    repo_root().join("examples").join("agents")
}

fn website_root() -> Option<PathBuf> {
    let dir = repo_root().parent()?.join("miri-lang.org");
    dir.is_dir().then_some(dir)
}

fn strings(value: &toml::Value) -> Vec<String> {
    value
        .as_array()
        .expect("expected an array of strings")
        .iter()
        .map(|v| v.as_str().expect("expected a string").to_string())
        .collect()
}

fn sessions() -> Vec<Session> {
    let text = fs::read_to_string(examples_dir().join("runs.toml")).expect("read runs.toml");
    let table: toml::Table = text.parse().expect("runs.toml parses");
    table["session"]
        .as_array()
        .expect("runs.toml lists sessions")
        .iter()
        .map(|session| Session {
            id: session["id"].as_str().expect("session id").to_string(),
            files: strings(&session["files"]),
            steps: session["step"]
                .as_array()
                .expect("a session has steps")
                .iter()
                .map(|step| Step {
                    args: strings(&step["args"]),
                    exit: step["exit"].as_integer().expect("step exit") as i32,
                })
                .collect(),
        })
        .collect()
}

/// Drop the session directory from every path the compiler printed, in both
/// the spelling it was created under and its canonical one (`/private/var`
/// on macOS), and blank every `durationMs` value.
fn normalised(text: &str, dir: &Path) -> String {
    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let mut text = text.to_string();
    for prefix in [canonical.as_path(), dir] {
        text = text.replace(&format!("{}/", prefix.display()), "");
    }
    without_durations(&text)
}

fn without_durations(text: &str) -> String {
    const KEY: &str = "\"durationMs\": ";
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find(KEY) {
        out.push_str(&rest[..at + KEY.len()]);
        rest = rest[at + KEY.len()..].trim_start_matches(|c: char| c.is_ascii_digit());
        out.push('_');
    }
    out.push_str(rest);
    out
}

fn replay(session: &Session) -> Vec<Recorded> {
    let dir = tempfile::tempdir().expect("session directory");
    for file in &session.files {
        fs::copy(examples_dir().join(file), dir.path().join(file)).expect("copy example");
    }
    session
        .steps
        .iter()
        .map(|step| {
            let output = Command::new(assert_cmd::cargo_bin!("miri"))
                .args(&step.args)
                .current_dir(dir.path())
                .env_remove("NO_COLOR")
                .output()
                .expect("run miri");
            Recorded {
                args: step.args.clone(),
                exit: output.status.code().unwrap_or(-1),
                stdout: normalised(&String::from_utf8_lossy(&output.stdout), dir.path()),
                stderr: normalised(&String::from_utf8_lossy(&output.stderr), dir.path()),
            }
        })
        .collect()
}

/// Every command the guide shows still ends the way the guide says it does.
#[test]
fn test_every_command_the_agent_guide_shows_exits_as_recorded() {
    for session in sessions() {
        for (step, recorded) in session.steps.iter().zip(replay(&session)) {
            assert_eq!(
                recorded.exit, step.exit,
                "session `{}`: `miri {}` exited {} where the guide records {}\nstdout:\n{}\nstderr:\n{}",
                session.id,
                step.args.join(" "),
                recorded.exit,
                step.exit,
                recorded.stdout,
                recorded.stderr
            );
        }
    }
}

/// The guide recommends only surfaces the published packs list, plus the
/// `skill` command that installs those packs.
#[test]
fn test_the_agent_guide_runs_only_listed_surfaces() {
    let text = fs::read_to_string(repo_root().join("skills").join("surfaces.toml"))
        .expect("read surfaces.toml");
    let table: toml::Table = text.parse().expect("surfaces.toml parses");
    let listed = strings(&table["commands"]);
    for session in sessions() {
        for step in &session.steps {
            let command = format!("miri {}", step.args[0]);
            assert!(
                listed.contains(&command) || command == "miri skill",
                "session `{}` runs `{command}`, which skills/surfaces.toml does not list",
                session.id
            );
        }
    }
}

fn read_json(path: &Path) -> serde_json::Value {
    let text = fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("{} is missing ({e}); run gen_agent_docs.py", path.display()));
    serde_json::from_str(&text).expect("valid JSON")
}

fn file_names(dir: &Path, keep: impl Fn(&Path) -> bool) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read directory")
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| keep(path))
        .filter_map(|path| path.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    names.sort();
    names
}

/// The vendored texts under `kind` are exactly `expected`, by name and content.
fn assert_vendored_texts(sources: &serde_json::Value, kind: &str, expected: &[(String, PathBuf)]) {
    let vendored = sources[kind].as_object().cloned().unwrap_or_default();
    let mut vendored_names: Vec<&String> = vendored.keys().collect();
    vendored_names.sort();
    let expected_names: Vec<&String> = expected.iter().map(|(name, _)| name).collect();
    assert_eq!(
        vendored_names, expected_names,
        "the website vendors a different set of {kind}; run gen_agent_docs.py"
    );
    for (name, path) in expected {
        let source = fs::read_to_string(path).expect("read source");
        assert!(
            vendored[name].as_str() == Some(source.as_str()),
            "the website's copy of {} differs from the repository's; run gen_agent_docs.py",
            path.display()
        );
    }
}

fn assert_vendored_sources_match(site: &Path) {
    let sources = read_json(&site.join("_data").join("agent_sources.json"));
    let skills_dir = repo_root().join("skills");
    let packs: Vec<(String, PathBuf)> = file_names(&skills_dir, |p| p.join("SKILL.md").is_file())
        .into_iter()
        .map(|name| {
            let path = skills_dir.join(&name).join("SKILL.md");
            (name, path)
        })
        .collect();
    assert_vendored_texts(&sources, "skills", &packs);
    let examples: Vec<(String, PathBuf)> = file_names(&examples_dir(), |p| {
        p.extension().is_some_and(|e| e == "mi")
    })
    .into_iter()
    .map(|name| {
        let path = examples_dir().join(&name);
        (name, path)
    })
    .collect();
    assert_vendored_texts(&sources, "examples", &examples);
}

fn assert_registry_matches(site: &Path) {
    let output = Command::new(assert_cmd::cargo_bin!("miri"))
        .args(["explain", "--list", "--format", "json"])
        .output()
        .expect("run miri explain --list");
    let fresh: serde_json::Value = serde_json::from_slice(&output.stdout).expect("registry JSON");
    assert!(
        read_json(&site.join("_data").join("agent_codes.json")) == fresh,
        "the website's code registry is stale; run gen_agent_docs.py"
    );
    assert!(
        read_json(&site.join("_data").join("diagnostics_schema.json"))
            == read_json(&repo_root().join("docs").join("diagnostics-schema.json")),
        "the website's copy of the diagnostics schema is stale; run gen_agent_docs.py"
    );
}

fn recorded_from_json(step: &serde_json::Value) -> Recorded {
    let text = |key: &str| step[key].as_str().unwrap_or_default().to_string();
    Recorded {
        args: step["args"]
            .as_array()
            .map(|args| {
                args.iter()
                    .filter_map(|a| a.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default(),
        exit: step["exit"].as_i64().unwrap_or(-1) as i32,
        stdout: without_durations(&text("stdout")),
        stderr: without_durations(&text("stderr")),
    }
}

fn assert_recorded_runs_match(site: &Path) {
    let published = read_json(&site.join("_data").join("agent_runs.json"));
    let all = sessions();
    let ids: Vec<&str> = all.iter().map(|s| s.id.as_str()).collect();
    let published_ids: Vec<&str> = published
        .as_object()
        .map(|o| o.keys().map(String::as_str).collect())
        .unwrap_or_default();
    let mut sorted_ids = ids.clone();
    sorted_ids.sort();
    let mut sorted_published = published_ids.clone();
    sorted_published.sort();
    assert_eq!(
        sorted_published, sorted_ids,
        "the website records a different set of sessions; run gen_agent_docs.py"
    );
    for session in &all {
        let recorded: Vec<Recorded> = published[&session.id]
            .as_array()
            .map(|steps| steps.iter().map(recorded_from_json).collect())
            .unwrap_or_default();
        assert_eq!(
            recorded,
            replay(session),
            "session `{}` prints something other than the website records; run gen_agent_docs.py",
            session.id
        );
    }
}

/// The website's vendored copies match a fresh render of this repository.
#[test]
fn test_the_website_agent_guide_matches_a_fresh_render() {
    let Some(site) = website_root() else {
        eprintln!("skipping: ../miri-lang.org is not checked out next to this repository");
        return;
    };
    assert_vendored_sources_match(&site);
    assert_registry_matches(&site);
    assert_recorded_runs_match(&site);
}
