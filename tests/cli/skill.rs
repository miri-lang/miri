// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! `miri skill` — the agent skills this build carries.

use std::path::{Path, PathBuf};

use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand};

use crate::utils::miri_cmd;

/// Run the command and return what it wrote and whether it succeeded.
fn run(args: &[&str]) -> (String, String, bool) {
    let output = miri_cmd()
        .arg("skill")
        .args(args)
        .output()
        .expect("the skill command runs");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// Parse the envelope out of a JSON run.
fn envelope(text: &str) -> DiagnosticsEnvelope {
    serde_json::from_str(text)
        .unwrap_or_else(|error| panic!("{} is not an envelope: {}", text, error))
}

/// The repository's own copy of a skill, which the binary was built from.
fn source_of(name: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("skills")
        .join(name)
        .join("SKILL.md");
    std::fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} is readable", path.display()))
}

/// Everything after the header block, which is the text a reader copies.
pub fn body_of(text: &str) -> &str {
    let opened = text
        .strip_prefix("---\n")
        .expect("a skill starts with a header");
    let close = opened.find("\n---\n").expect("the header closes");
    &opened[close + 5..]
}

/// Read a header field out of a skill's header block.
fn field_of(text: &str, field: &str) -> Option<String> {
    let opened = text.strip_prefix("---\n")?;
    let close = opened.find("\n---\n")?;
    opened[..close].lines().find_map(|line| {
        line.trim()
            .strip_prefix(field)?
            .strip_prefix(':')
            .map(|value| value.trim().to_string())
    })
}

/// The version this binary reports, without the platform it was built for.
fn compiler_version() -> String {
    let output = miri_cmd()
        .arg("--version")
        .output()
        .expect("the version command runs");
    let printed = String::from_utf8_lossy(&output.stdout).into_owned();
    printed
        .split_whitespace()
        .nth(1)
        .expect("the version line names a version")
        .to_string()
}

/// A directory to install into, removed when the test ends.
fn workspace(name: &str) -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix(&format!("miri-skill-{}-", name))
        .tempdir()
        .expect("a temporary directory should be available")
}

/// The names of the skill directories this repository carries.
fn source_names() -> Vec<String> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let mut names: Vec<String> = std::fs::read_dir(&root)
        .expect("the skills directory is readable")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            entry
                .path()
                .join("SKILL.md")
                .exists()
                .then(|| entry.file_name().to_string_lossy().into_owned())
        })
        .collect();
    names.sort();
    names
}

#[test]
fn test_list_names_every_skill_with_its_description() {
    let (stdout, _, ok) = run(&["list"]);
    assert!(ok, "listing the catalogue succeeds");

    for name in source_names() {
        let description = field_of(&source_of(&name), "description")
            .unwrap_or_else(|| panic!("{} declares a description", name));
        assert!(
            stdout.contains(&name) && stdout.contains(&description),
            "the listing is missing {}",
            name
        );
    }
}

#[test]
fn test_every_skill_in_the_repository_is_carried_by_the_binary() {
    let (stdout, _, ok) = run(&["list"]);
    assert!(ok, "listing the catalogue succeeds");

    // A skill added under `skills/` reaches an agent only by being embedded.
    // Without this check the embedded list falls silently behind the sources.
    for name in source_names() {
        assert!(
            stdout.contains(&name),
            "{} exists under skills/ but this build does not carry it",
            name
        );
    }
}

#[test]
fn test_list_as_json_carries_the_catalogue_in_an_envelope() {
    let (stdout, _, ok) = run(&["list", "--format", "json"]);
    assert!(ok, "listing the catalogue succeeds");

    let envelope = envelope(&stdout);
    assert_eq!(envelope.command, JsonCommand::Skill);
    assert!(envelope.ok);
    assert_eq!(envelope.schema_version, 1);

    let skills = envelope.skills.expect("the envelope carries the catalogue");
    // The catalogue is ordered for a reader; the directory listing is not, so
    // the two are compared on which skills they name rather than on order.
    let mut listed: Vec<String> = skills.iter().map(|skill| skill.name.clone()).collect();
    listed.sort();
    assert_eq!(listed, source_names());
    for skill in &skills {
        assert_eq!(skill.compiler_version, compiler_version());
        assert!(skill.installed_path.is_none(), "nothing was installed");
        assert!(skill.body.is_none(), "a listing does not carry bodies");
    }
}

#[test]
fn test_show_reproduces_the_body_it_was_built_from_byte_for_byte() {
    let (stdout, _, ok) = run(&["show", "miri-lang"]);
    assert!(ok, "showing a skill succeeds");

    // The point of bundling: what an agent reads is what this build carries,
    // not a rendering of it.
    assert_eq!(body_of(&stdout), body_of(&source_of("miri-lang")));
}

#[test]
fn test_show_stamps_the_version_the_repository_source_does_not_carry() {
    let (stdout, _, ok) = run(&["show", "miri-lang"]);
    assert!(ok, "showing a skill succeeds");

    assert_eq!(
        field_of(&stdout, "compilerVersion"),
        Some(compiler_version())
    );
    assert!(
        !source_of("miri-lang").contains("compilerVersion"),
        "the version is stamped when the skill is written out, not stored in the source"
    );
}

#[test]
fn test_showing_a_name_that_is_not_carried_is_refused() {
    let (stdout, stderr, ok) = run(&["show", "not-a-skill"]);
    assert!(!ok, "an unknown name fails");
    assert!(
        stdout.is_empty(),
        "nothing is written where content would go"
    );
    assert!(stderr.contains("MER_BLD_013"), "stderr was: {}", stderr);
}

#[test]
fn test_a_refused_name_reports_as_json_when_json_was_asked_for() {
    let (_, stderr, ok) = run(&["show", "not-a-skill", "--format", "json"]);
    assert!(!ok, "an unknown name fails");

    let envelope = envelope(&stderr);
    assert!(!envelope.ok);
    assert_eq!(envelope.diagnostics[0].code.as_deref(), Some("MER_BLD_013"));
}

/// The file `--agent claude` writes for `name` under `root`.
fn claude_path(root: &Path, name: &str) -> PathBuf {
    root.join(".claude/skills").join(name).join("SKILL.md")
}

#[test]
fn test_install_writes_every_skill_where_the_agent_reads_it() {
    let workspace = workspace("install");
    let root = workspace.path().to_string_lossy().into_owned();
    let (_, stderr, ok) = run(&["install", "--agent", "claude", "--target", &root]);
    assert!(ok, "installing succeeds: {}", stderr);

    for name in source_names() {
        let written = std::fs::read_to_string(claude_path(workspace.path(), &name))
            .unwrap_or_else(|_| panic!("{} was written", name));
        assert_eq!(body_of(&written), body_of(&source_of(&name)));
        assert_eq!(
            field_of(&written, "compilerVersion"),
            Some(compiler_version())
        );
    }
}

#[test]
#[cfg(unix)]
fn test_installing_over_an_identical_file_writes_nothing() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = workspace("identical");
    let root = workspace.path().to_string_lossy().into_owned();
    let args = [
        "install",
        "miri-lang",
        "--agent",
        "claude",
        "--target",
        &root,
    ];
    assert!(run(&args).2, "the first install succeeds");

    // Printing "unchanged" proves nothing on its own — rewriting identical
    // bytes prints the same word. Taking write permission away makes the
    // claim testable: the run can only succeed by not writing.
    let path = claude_path(workspace.path(), "miri-lang");
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o444))
        .expect("the file can be made read-only");

    let (stdout, stderr, ok) = run(&args);
    assert!(ok, "installing the same text again succeeds: {}", stderr);
    assert!(stdout.contains("unchanged"), "stdout was: {}", stdout);

    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644))
        .expect("the file can be made writable again for cleanup");
}

#[test]
fn test_installing_over_an_edited_file_is_refused_until_forced() {
    let workspace = workspace("edited");
    let root = workspace.path().to_string_lossy().into_owned();
    let args = [
        "install",
        "miri-lang",
        "--agent",
        "claude",
        "--target",
        &root,
    ];
    assert!(run(&args).2, "the first install succeeds");

    let path = claude_path(workspace.path(), "miri-lang");
    let installed = std::fs::read_to_string(&path).expect("the skill was written");
    std::fs::write(&path, format!("{}\nlocal note\n", installed)).expect("the edit is written");

    let (_, stderr, ok) = run(&args);
    assert!(!ok, "an edited file is not silently discarded");
    assert!(stderr.contains("MER_BLD_014"), "stderr was: {}", stderr);
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file survives"),
        format!("{}\nlocal note\n", installed),
        "a refusal leaves the edit in place"
    );

    let mut forced = args.to_vec();
    forced.push("--force");
    assert!(run(&forced).2, "forcing succeeds");
    assert_eq!(
        std::fs::read_to_string(&path).expect("the file was replaced"),
        installed
    );
}

#[test]
fn test_a_refusal_still_reports_the_skills_that_were_written() {
    let workspace = workspace("partial");
    let root = workspace.path().to_string_lossy().into_owned();
    let args = ["install", "--agent", "claude", "--target", &root];
    assert!(run(&args).2, "the first install succeeds");

    let edited = claude_path(workspace.path(), "miri-gpu");
    let installed = std::fs::read_to_string(&edited).expect("the skill was written");
    std::fs::write(&edited, format!("{}\nlocal note\n", installed)).expect("the edit is written");

    let mut json = args.to_vec();
    json.extend(["--format", "json"]);
    let (stdout, _, ok) = run(&json);
    assert!(!ok, "one refusal fails the call");

    // A half-installed set reported as success is how a stale skill survives.
    let envelope = envelope(&stdout);
    assert!(!envelope.ok);
    assert_eq!(envelope.diagnostics[0].code.as_deref(), Some("MER_BLD_014"));
    let written: Vec<String> = envelope
        .skills
        .expect("the envelope names what was written")
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    assert!(written.contains(&"miri-lang".to_string()));
    assert!(!written.contains(&"miri-gpu".to_string()));
}

#[test]
fn test_each_flavor_writes_where_the_tools_it_names_read() {
    let workspace = workspace("flavors");
    let root = workspace.path().to_string_lossy().into_owned();

    // Cursor, Codex and the tools beside them read one shared directory;
    // Claude Code reads only its own.
    for flavor in ["agents", "cursor", "codex"] {
        let cleaned = workspace.path().join(".agents");
        let _ = std::fs::remove_dir_all(&cleaned);
        let (_, stderr, ok) = run(&["install", "miri-lang", "--agent", flavor, "--target", &root]);
        assert!(ok, "installing for {} succeeds: {}", flavor, stderr);
        assert!(
            workspace
                .path()
                .join(".agents/skills/miri-lang/SKILL.md")
                .exists(),
            "{} did not write the shared path",
            flavor
        );
    }

    assert!(
        run(&[
            "install",
            "miri-lang",
            "--agent",
            "generic",
            "--target",
            &root
        ])
        .2
    );
    assert!(workspace.path().join("skills/miri-lang/SKILL.md").exists());
}

#[test]
#[cfg(unix)]
fn test_a_target_that_cannot_be_written_is_reported() {
    use std::os::unix::fs::PermissionsExt;

    let workspace = workspace("readonly");
    let closed = workspace.path().join("closed");
    std::fs::create_dir(&closed).expect("the directory is created");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o500))
        .expect("the directory can be closed for writing");

    // A user running as root writes regardless of the mode bits, so the check
    // would prove nothing there.
    if std::fs::create_dir(closed.join("probe")).is_ok() {
        return;
    }

    let root = closed.to_string_lossy().into_owned();
    let (_, stderr, ok) = run(&[
        "install",
        "miri-lang",
        "--agent",
        "claude",
        "--target",
        &root,
    ]);
    assert!(!ok, "an unwritable target fails");
    assert!(stderr.contains("MER_BLD_015"), "stderr was: {}", stderr);

    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o700))
        .expect("the directory can be reopened for cleanup");
}

#[test]
fn test_a_target_that_is_a_file_rather_than_a_directory_is_reported() {
    let workspace = workspace("target-is-file");
    let file = workspace.path().join("not-a-directory");
    std::fs::write(&file, "").expect("the file is created");

    let root = file.to_string_lossy().into_owned();
    let (_, stderr, ok) = run(&[
        "install",
        "miri-lang",
        "--agent",
        "claude",
        "--target",
        &root,
    ]);
    assert!(!ok, "a target that cannot hold a directory fails");
    assert!(stderr.contains("MER_BLD_015"), "stderr was: {}", stderr);
}

#[test]
fn test_an_unknown_name_beside_known_ones_installs_the_rest_and_still_fails() {
    let workspace = workspace("mixed-names");
    let root = workspace.path().to_string_lossy().into_owned();
    let (stdout, _, ok) = run(&[
        "install",
        "miri-lang",
        "not-a-skill",
        "miri-gpu",
        "--agent",
        "claude",
        "--target",
        &root,
        "--format",
        "json",
    ]);
    assert!(!ok, "one unknown name fails the call");

    let envelope = envelope(&stdout);
    assert!(!envelope.ok);
    assert_eq!(envelope.diagnostics[0].code.as_deref(), Some("MER_BLD_013"));

    // The skills that could be written are written, and are named as such.
    let written: Vec<String> = envelope
        .skills
        .expect("the envelope names what was written")
        .iter()
        .map(|skill| skill.name.clone())
        .collect();
    assert_eq!(
        written,
        vec!["miri-lang".to_string(), "miri-gpu".to_string()]
    );
    assert!(claude_path(workspace.path(), "miri-lang").exists());
    assert!(claude_path(workspace.path(), "miri-gpu").exists());
}
