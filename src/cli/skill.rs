// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri skill` command: the agent skills this build carries.
//!
//! The skills are compiled into the binary, so the guidance an agent reads
//! cannot drift from the language the binary accepts. A skill is a markdown
//! file with a small YAML header; the header names it, and everything after
//! the header is prose the compiler never interprets.
//!
//! Nothing here rewrites that prose. The header is re-emitted with the
//! compiler's version stamped into it, and the body is copied byte for byte,
//! so what an agent reads is what this build was made from.
//!
//! The module is split the way [`crate::cli::view`] is: the functions that
//! compute a result return it without printing, so a long-lived server can
//! call them, and the `run_*` functions add the writing for the command line.

use std::path::{Path, PathBuf};

use crate::cli::args::AgentFlavor;
use crate::cli::{coded, sanitize_for_terminal, serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonSkill};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{to_json, Diagnostic};
use crate::error::format::format_diagnostic_with_color;

const MIRI_LANG: &str = include_str!("../../skills/miri-lang/SKILL.md");
const MIRI_GPU: &str = include_str!("../../skills/miri-gpu/SKILL.md");
const MIRI_TESTING: &str = include_str!("../../skills/miri-testing/SKILL.md");

/// The skills this build carries, paired with the name each is installed under.
///
/// A skill added under `skills/` reaches the binary only by being listed here.
/// A test walks that directory and fails when an entry is missing, so the list
/// cannot fall behind the sources without the suite saying so.
const EMBEDDED: &[(&str, &str)] = &[
    ("miri-lang", MIRI_LANG),
    ("miri-gpu", MIRI_GPU),
    ("miri-testing", MIRI_TESTING),
];

/// How the command finished, mapped onto a process exit code by the caller.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// The request was answered.
    Done,
    /// The request could not be answered.
    Failed,
}

impl Outcome {
    /// The exit code this outcome ends the process with.
    pub fn exit_code(self) -> i32 {
        match self {
            Outcome::Done => 0,
            Outcome::Failed => 1,
        }
    }
}

/// One skill from the catalogue.
#[derive(Debug)]
pub struct Skill {
    /// The name it is installed under, which its header must agree with.
    pub name: &'static str,
    /// The one line that tells an agent when to reach for it.
    pub description: &'static str,
    /// Everything after the header, exactly as it was written.
    pub body: &'static str,
    /// The whole file as it was embedded.
    source: &'static str,
}

impl Skill {
    /// The text to write out: the header carrying this build's version, then
    /// the body unchanged.
    pub fn stamped(&self) -> Result<String, Box<Diagnostic>> {
        stamp_version(self.source, crate::cli::crate_version()).ok_or_else(|| malformed(self.name))
    }
}

/// What a call to the command produced.
pub struct SkillReport {
    /// The envelope, ready to serialize for a machine consumer.
    pub envelope: DiagnosticsEnvelope,
    /// Whether every requested skill was handled.
    pub ok: bool,
    /// The lines describing what happened, for a person to read.
    pub summary: Vec<String>,
    /// The diagnostics as the compiler reported them.
    diagnostics: Vec<Diagnostic>,
}

impl SkillReport {
    /// Render this report for a person to read.
    pub fn to_pretty(&self, color_mode: ColorMode) -> String {
        if self.diagnostics.is_empty() {
            return self.summary.join("\n");
        }
        self.diagnostics
            .iter()
            .map(|diagnostic| format_diagnostic_with_color("", diagnostic, None, color_mode.into()))
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// Split a skill file into its header and the body that follows it.
///
/// The body is returned as a slice of the input rather than as rebuilt lines,
/// which is what lets the text an agent reads stay byte-identical to the
/// source this build was made from.
fn split_header(source: &str) -> Option<(&str, &str)> {
    let after_open = source
        .strip_prefix("---\n")
        .or_else(|| source.strip_prefix("---\r\n"))?;

    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        if line.trim_end() == "---" {
            return Some((&after_open[..offset], &after_open[offset + line.len()..]));
        }
        offset += line.len();
    }
    None
}

/// Read a header field, which is a name, a colon, and the rest of the line.
fn header_field<'a>(header: &'a str, field: &str) -> Option<&'a str> {
    header.lines().find_map(|line| {
        line.trim()
            .strip_prefix(field)?
            .strip_prefix(':')
            .map(str::trim)
    })
}

/// Read one embedded skill, checking that its header agrees with its name.
fn read(name: &'static str, source: &'static str) -> Result<Skill, Box<Diagnostic>> {
    let (header, body) = split_header(source).ok_or_else(|| malformed(name))?;
    let declared = header_field(header, "name").unwrap_or_default();
    let description = header_field(header, "description").ok_or_else(|| malformed(name))?;

    if declared != name || description.is_empty() {
        return Err(malformed(name));
    }

    Ok(Skill {
        name,
        description,
        body,
        source,
    })
}

/// Every skill this build carries, in the order they are listed.
pub fn catalogue() -> Result<Vec<Skill>, Box<Diagnostic>> {
    EMBEDDED
        .iter()
        .map(|(name, source)| read(name, source))
        .collect()
}

/// The one skill that goes by this name.
pub fn find(name: &str) -> Result<Skill, Box<Diagnostic>> {
    let (embedded_name, source) = EMBEDDED
        .iter()
        .find(|(embedded_name, _)| *embedded_name == name)
        .ok_or_else(|| unknown_skill(name))?;
    read(embedded_name, source)
}

/// Re-emit a skill's header with `version` recorded in it.
///
/// The header is rebuilt a line at a time so the version lands next to the
/// fields it belongs with; the body is appended untouched.
fn stamp_version(source: &str, version: &str) -> Option<String> {
    let (header, body) = split_header(source)?;

    // A file written with Windows line endings keeps them, so the text that
    // comes back differs from the text that went in only by the version line.
    let ending = if source.starts_with("---\r\n") {
        "\r\n"
    } else {
        "\n"
    };
    let stamp = format!("compilerVersion: {}{}", version, ending);
    let delimiter = format!("---{}", ending);

    let mut out = String::with_capacity(source.len() + stamp.len());
    out.push_str(&delimiter);
    let mut stamped = false;
    for line in header.split_inclusive('\n') {
        if line.trim_end().starts_with("compilerVersion:") {
            continue;
        }
        out.push_str(line);
        if line.trim_end().starts_with("description:") && !stamped {
            out.push_str(&stamp);
            stamped = true;
        }
    }
    if !stamped {
        out.push_str(&stamp);
    }
    out.push_str(&delimiter);
    out.push_str(body);
    Some(out)
}

/// Where a skill goes for the agent that will read it.
///
/// Claude Code reads `.claude/skills`. Cursor, Codex, OpenCode, Windsurf and
/// Gemini CLI all read `.agents/skills`, so the flavors naming those tools
/// share that one path rather than each getting a directory only they look in.
fn install_path(root: &Path, flavor: AgentFlavor, name: &str) -> PathBuf {
    let directory = match flavor {
        AgentFlavor::Claude => ".claude/skills",
        AgentFlavor::Agents | AgentFlavor::Cursor | AgentFlavor::Codex => ".agents/skills",
        AgentFlavor::Generic => "skills",
    };
    root.join(directory).join(name).join("SKILL.md")
}

/// Report a name that is not in the catalogue.
fn unknown_skill(name: &str) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldUnknownSkill,
        format!("no skill named `{}`", sanitize_for_terminal(name)),
        "`miri skill list` names every skill this build carries",
    )
}

/// Report an embedded skill whose header cannot be read.
fn malformed(name: &str) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldSkillSourceMalformed,
        format!("the skill `{}` has no readable header", name),
        "a skill starts with a `---` block naming it and describing when to use it",
    )
}

/// Assemble a report from what was produced and what went wrong.
fn report(
    installed: Vec<JsonSkill>,
    diagnostics: Vec<Diagnostic>,
    summary: Vec<String>,
) -> SkillReport {
    let ok = diagnostics.is_empty();
    let json = diagnostics
        .iter()
        .map(|diagnostic| to_json(diagnostic, "", None))
        .collect::<Vec<JsonDiagnostic>>();

    let mut envelope = DiagnosticsEnvelope::new(JsonCommand::Skill, ok, json)
        .with_exit_code(if ok { 0 } else { 1 });
    if !installed.is_empty() {
        envelope = envelope.with_skills(installed);
    }

    SkillReport {
        envelope,
        ok,
        summary,
        diagnostics,
    }
}

/// Describe every skill this build carries.
pub fn list() -> SkillReport {
    match catalogue() {
        Ok(skills) => {
            let summary = skills
                .iter()
                .map(|skill| format!("{}  {}", skill.name, skill.description))
                .collect();
            let listed = skills.iter().map(|skill| describe(skill, None)).collect();
            report(listed, vec![], summary)
        }
        Err(diagnostic) => report(vec![], vec![*diagnostic], vec![]),
    }
}

/// Render one skill's entry in the envelope.
fn describe(skill: &Skill, installed_path: Option<String>) -> JsonSkill {
    JsonSkill {
        name: skill.name.to_string(),
        description: skill.description.to_string(),
        compiler_version: crate::cli::crate_version().to_string(),
        installed_path,
        unchanged: None,
        body: None,
    }
}

/// The text `miri skill show` writes: the stamped header and the body.
pub fn show(name: &str) -> Result<String, Box<Diagnostic>> {
    find(name)?.stamped()
}

/// What installing one skill did to the file it targets.
enum Written {
    /// The file was created or replaced.
    Wrote,
    /// The file already held exactly this text.
    Unchanged,
}

/// Write one skill, refusing to discard an edited file.
fn install_one(
    skill: &Skill,
    root: &Path,
    flavor: AgentFlavor,
    force: bool,
) -> Result<(PathBuf, Written), Box<Diagnostic>> {
    let text = skill.stamped()?;
    let path = install_path(root, flavor, skill.name);

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| not_writable(parent, &error))?;
    }

    if let Ok(existing) = std::fs::read_to_string(&path) {
        if existing == text {
            return Ok((path, Written::Unchanged));
        }
        if !force {
            return Err(locally_modified(&path));
        }
    }

    std::fs::write(&path, &text).map_err(|error| not_writable(&path, &error))?;
    Ok((path, Written::Wrote))
}

/// Report a path this process cannot write to.
fn not_writable(path: &Path, error: &std::io::Error) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldOutputNotWritable,
        format!(
            "cannot write `{}`: {}",
            sanitize_for_terminal(&path.display().to_string()),
            error
        ),
        "check that the directory exists and this user may write to it",
    )
}

/// Report an installed file that no longer matches what this build writes.
fn locally_modified(path: &Path) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldSkillLocallyModified,
        format!(
            "`{}` differs from the skill this build carries",
            sanitize_for_terminal(&path.display().to_string())
        ),
        "`--force` replaces the file and discards the local edits",
    )
}

/// Write the named skills, or every skill when none are named.
pub fn install(names: &[String], flavor: AgentFlavor, root: &Path, force: bool) -> SkillReport {
    let mut installed = Vec::new();
    let mut diagnostics = Vec::new();
    let mut summary = Vec::new();

    for name in requested(names) {
        match find(name) {
            Err(diagnostic) => diagnostics.push(*diagnostic),
            Ok(skill) => match install_one(&skill, root, flavor, force) {
                Err(diagnostic) => diagnostics.push(*diagnostic),
                Ok((path, written)) => {
                    let unchanged = matches!(written, Written::Unchanged);
                    let shown = path.display().to_string();
                    summary.push(format!(
                        "{} {}",
                        if unchanged { "unchanged" } else { "wrote" },
                        sanitize_for_terminal(&shown)
                    ));
                    let mut entry = describe(&skill, Some(shown));
                    entry.unchanged = Some(unchanged);
                    installed.push(entry);
                }
            },
        }
    }

    report(installed, diagnostics, summary)
}

/// The names to act on: the ones asked for, or all of them.
fn requested(names: &[String]) -> Vec<&str> {
    if names.is_empty() {
        EMBEDDED.iter().map(|(name, _)| *name).collect()
    } else {
        names.iter().map(String::as_str).collect()
    }
}

/// Answer a `skillsGet` request with the skills it asked for.
///
/// The body travels with each entry, because the caller is a tool that cannot
/// read this binary's files for itself.
pub fn agent_envelope(name: Option<&str>) -> DiagnosticsEnvelope {
    let found = match name {
        Some(name) => find(name).map(|skill| vec![skill]),
        None => catalogue(),
    };

    match found {
        Ok(skills) => {
            let entries = skills
                .iter()
                .map(|skill| {
                    let mut entry = describe(skill, None);
                    entry.body = Some(skill.body.to_string());
                    entry
                })
                .collect();
            report(entries, vec![], vec![]).envelope
        }
        Err(diagnostic) => report(vec![], vec![*diagnostic], vec![]).envelope,
    }
}

/// Name every skill this build carries.
pub fn run_list(format: Format, color_mode: ColorMode) -> Outcome {
    write_report(&list(), format, color_mode)
}

/// Write one skill to standard output.
///
/// The output is the file itself, so it can be redirected into place. A
/// failure goes to standard error, where it will not be mistaken for content.
pub fn run_show(name: &str, format: Format, color_mode: ColorMode) -> Outcome {
    match show(name) {
        Ok(text) => {
            print!("{}", text);
            Outcome::Done
        }
        Err(diagnostic) => {
            let failed = report(vec![], vec![*diagnostic], vec![]);
            match format {
                Format::Json => eprintln!("{}", serialize_envelope(&failed.envelope)),
                Format::Pretty => eprintln!("{}", failed.to_pretty(color_mode)),
            }
            Outcome::Failed
        }
    }
}

/// Write the named skills into place.
pub fn run_install(
    names: &[String],
    flavor: AgentFlavor,
    root: &Path,
    force: bool,
    format: Format,
    color_mode: ColorMode,
) -> Outcome {
    write_report(&install(names, flavor, root, force), format, color_mode)
}

/// Print a report in the format the caller asked for.
fn write_report(report: &SkillReport, format: Format, color_mode: ColorMode) -> Outcome {
    match format {
        Format::Json => println!("{}", serialize_envelope(&report.envelope)),
        Format::Pretty => {
            let summary = report.summary.join("\n");
            if !summary.is_empty() {
                println!("{}", summary);
            }
            if !report.ok {
                eprintln!("{}", report.to_pretty(color_mode));
            }
        }
    }

    if report.ok {
        Outcome::Done
    } else {
        Outcome::Failed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_embedded_skill_has_a_readable_header() {
        let skills = catalogue().expect("every embedded skill parses");
        assert_eq!(skills.len(), EMBEDDED.len());
        for skill in skills {
            assert!(
                !skill.description.is_empty(),
                "{} has no description",
                skill.name
            );
        }
    }

    #[test]
    fn stamping_leaves_the_body_untouched() {
        for skill in catalogue().expect("every embedded skill parses") {
            let stamped = skill.stamped().expect("a parsed skill stamps");
            let (_, body) = split_header(&stamped).expect("the stamped text has a header");
            assert_eq!(body, skill.body, "{} lost its body", skill.name);
        }
    }

    #[test]
    fn stamping_replaces_a_version_rather_than_repeating_it() {
        let source = "---\nname: n\ndescription: d\ncompilerVersion: old\n---\nbody\n";
        let stamped = stamp_version(source, "new").expect("a header with a version stamps");
        assert_eq!(stamped.matches("compilerVersion:").count(), 1);
        assert!(stamped.contains("compilerVersion: new\n"));
        assert!(stamped.ends_with("---\nbody\n"));
    }

    #[test]
    fn a_file_without_a_header_is_malformed() {
        assert!(split_header("no header here\n").is_none());
        assert!(split_header("---\nname: n\nnever closed\n").is_none());
    }

    #[test]
    fn a_header_that_disagrees_with_the_name_is_malformed() {
        // The header and the directory are two claims about what a reader is
        // getting, so they are not allowed to differ.
        let mismatched = "---\nname: other\ndescription: d\n---\nbody\n";
        let reported = read(
            "miri-lang",
            Box::leak(mismatched.to_string().into_boxed_str()),
        )
        .expect_err("a mismatched name is refused");
        assert_eq!(reported.code, Some("MER_BLD_016"));

        let undescribed = "---\nname: miri-lang\n---\nbody\n";
        let reported = read(
            "miri-lang",
            Box::leak(undescribed.to_string().into_boxed_str()),
        )
        .expect_err("a header with no description is refused");
        assert_eq!(reported.code, Some("MER_BLD_016"));
    }

    #[test]
    fn a_file_written_with_windows_line_endings_keeps_its_body() {
        let source = "---\r\nname: n\r\ndescription: d\r\n---\r\nbody — text\r\n";
        let (_, body) = split_header(source).expect("a header closes");
        let stamped = stamp_version(source, "1.0").expect("it stamps");
        let (_, restamped) = split_header(&stamped).expect("the stamped text has a header");
        assert_eq!(restamped, body);
        assert!(stamped.contains("compilerVersion: 1.0\r\n"));
    }

    #[test]
    fn a_flavor_reading_the_neutral_path_installs_there() {
        let root = Path::new("/tmp/root");
        for flavor in [AgentFlavor::Agents, AgentFlavor::Cursor, AgentFlavor::Codex] {
            assert_eq!(
                install_path(root, flavor, "miri-lang"),
                Path::new("/tmp/root/.agents/skills/miri-lang/SKILL.md")
            );
        }
        assert_eq!(
            install_path(root, AgentFlavor::Claude, "miri-lang"),
            Path::new("/tmp/root/.claude/skills/miri-lang/SKILL.md")
        );
        assert_eq!(
            install_path(root, AgentFlavor::Generic, "miri-lang"),
            Path::new("/tmp/root/skills/miri-lang/SKILL.md")
        );
    }

    #[test]
    fn an_unknown_name_is_not_in_the_catalogue() {
        let reported = find("nope").expect_err("an unknown name is refused");
        assert_eq!(reported.code, Some("MER_BLD_013"));
    }
}
