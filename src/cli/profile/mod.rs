// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The `miri profile` command: build a program for the profiler, run it with
//! the runtime's sampler on, and report which source lines its CPU time went
//! to.
//!
//! The program is built as `--profile` builds it, which writes the line map,
//! into a directory of its own, and run there with `MIRI_PROFILE_OUT` naming
//! the record the runtime writes at exit. Each sample's first frame is
//! charged to the line it was compiled from, and every line of its stack is
//! charged inclusively. Files are named as the line map names them, never by
//! where they sit on this machine.
//!
//! This module separates the work ([`profile`]) from the writing ([`run`]),
//! so a long-lived server could invoke the work function later.

mod line_table;
mod record;
mod report;

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::cli::{coded, output_tail, serialize_envelope, ColorMode, Format};
use crate::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic, JsonProfile};
use crate::diagnostics::DiagnosticCode;
use crate::error::diagnostic::{to_json, Diagnostic};
use crate::error::format::format_diagnostic_with_color;
use crate::error::CompilerError;
use crate::pipeline::{
    CodegenMode, Listing, Notices, Pipeline, Reported, RunCaptureResult, RunSetup,
    RUN_EXECUTABLE_NAME,
};
use line_table::{LineTable, SourceLine};
use record::ProfileRecord;
use report::{Attribution, ReportShape};

/// How many lines and functions a report lists unless asked otherwise.
pub const DEFAULT_TOP: usize = 20;

/// The samples per second of CPU time a run takes unless asked otherwise.
pub const DEFAULT_HZ: u32 = 1000;

/// The most samples per second a run may be asked for, as the runtime's
/// sampler accepts.
pub const MAX_HZ: u32 = 10_000;

/// Whether the runtime's sampler runs on this host: it reads interrupted
/// registers and arms a CPU-time timer only on macOS and Linux, on AArch64
/// and x86-64.
pub const HOST_HAS_SAMPLER: bool = cfg!(all(
    any(target_os = "macos", target_os = "linux"),
    any(target_arch = "aarch64", target_arch = "x86_64")
));

/// The name the record is written under in the run's directory.
const RECORD_NAME: &str = "profile";

/// The most bytes of the program's output an envelope carries.
const OUTPUT_TAIL_BYTES: usize = 8192;

/// What a profile asks for beyond the program.
#[derive(Debug, Clone, Copy)]
pub struct ProfileSettings {
    /// The most lines, and the most functions, the report lists.
    pub top: usize,
    /// The samples per second of CPU time.
    pub hz: u32,
}

/// A program to profile and how.
#[derive(Debug, Clone, Copy)]
pub struct ProfileRequest<'r> {
    /// The program's source file.
    pub path: &'r Path,
    /// The arguments the program is started with.
    pub program_args: &'r [String],
    pub settings: ProfileSettings,
    /// Whether the build runs the MIR verification pass.
    pub verify_mir: bool,
}

/// What came of profiling a program that ran.
#[derive(Debug)]
pub enum Finding {
    /// Where the run's time went.
    Profile(JsonProfile),
    /// The run finished and left no samples to report.
    NoProfile(Box<Diagnostic>),
    /// The run died before it finished: why, then what it died of, and the
    /// samples it took up to then when it left any.
    Died {
        diagnostics: Vec<Diagnostic>,
        profile: Option<JsonProfile>,
    },
}

impl Finding {
    /// The report the run left, if any.
    fn profile(&self) -> Option<&JsonProfile> {
        match self {
            Finding::Profile(profile) => Some(profile),
            Finding::Died { profile, .. } => profile.as_ref(),
            Finding::NoProfile(_) => None,
        }
    }
}

/// How profiling went.
pub enum Verdict {
    /// This host has no sampler.
    Unsupported(Box<Diagnostic>),
    /// The program was not built or could not be started.
    NotRun(CompilerError),
    /// The program ran.
    Ran {
        run: RunCaptureResult,
        finding: Finding,
    },
}

/// Everything profiling a program produced, for a writer to report.
pub struct Profiled {
    pub pipeline: Pipeline,
    pub notices: Notices,
    pub verdict: Verdict,
    pub elapsed_ms: u64,
}

/// A pipeline that builds `path` as a profile build.
fn profile_pipeline(path: &Path, verify_mir: bool) -> Pipeline {
    let absolute = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let mut pipeline = Pipeline::new()
        .with_verify_mir(verify_mir)
        .with_run_codegen(CodegenMode::from_flags(false, true, None));
    if let Some(directory) = absolute.parent() {
        pipeline = pipeline.with_source_dir(directory.to_path_buf());
    }
    pipeline.with_source_path(absolute.display().to_string())
}

/// The diagnostic for a host with no sampler.
fn unsupported() -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldProfilerUnsupported,
        format!(
            "this host ({}-{}) has no sampling profiler; profiling runs on macOS and \
             Linux, on aarch64 and x86-64",
            std::env::consts::OS,
            std::env::consts::ARCH
        ),
        "profile on a macOS or Linux machine with an aarch64 or x86-64 processor",
    )
}

/// Build the requested program, `source`, for the profiler, run it
/// sampled, and report where its time went.
///
/// Nothing here writes to a stream or ends the process, so the same call
/// serves the command line and a request over a long-lived connection.
pub fn profile(request: &ProfileRequest<'_>, source: &str) -> Profiled {
    let start = std::time::Instant::now();
    let pipeline = profile_pipeline(request.path, request.verify_mir);
    let (notices, verdict) = if HOST_HAS_SAMPLER {
        sampled_run(&pipeline, source, request)
    } else {
        (Vec::new(), Verdict::Unsupported(unsupported()))
    };
    Profiled {
        pipeline,
        notices,
        verdict,
        elapsed_ms: start.elapsed().as_millis() as u64,
    }
}

/// Build and run the program in a directory of its own, with the sampler
/// writing its record there, and read what it left.
fn sampled_run(
    pipeline: &Pipeline,
    source: &str,
    request: &ProfileRequest<'_>,
) -> (Notices, Verdict) {
    let directory = match tempfile::tempdir() {
        Ok(directory) => directory,
        Err(error) => return (Vec::new(), Verdict::NotRun(CompilerError::Io(error))),
    };
    let record_path = directory.path().join(RECORD_NAME);
    let setup = RunSetup {
        work_dir: directory.path(),
        emit: vec![Listing::Lines],
        environment: vec![
            ("MIRI_PROFILE_OUT", record_path.clone().into_os_string()),
            (
                "MIRI_PROFILE_HZ",
                OsString::from(request.settings.hz.to_string()),
            ),
        ],
    };
    let Reported { outcome, notices } =
        pipeline.run_in_reported(source, request.program_args, &setup);
    let verdict = match outcome {
        Ok(run) => {
            let finding = finding_of(&run, directory.path(), pipeline, &request.settings);
            Verdict::Ran { run, finding }
        }
        Err(error) => Verdict::NotRun(error),
    };
    (notices, verdict)
}

/// Whether `run` died rather than finished: a trap, a signal, or no exit
/// status at all.
fn died(run: &RunCaptureResult) -> bool {
    let trapped = run.trap_code.is_some() && run.exit_code != 0;
    trapped || run.signal.is_some() || run.exit_code == NO_EXIT_STATUS
}

/// The status stood in for a program that finished without one of its own.
const NO_EXIT_STATUS: i32 = -1;

/// What a finished `run`, built and run in `directory`, left to report.
fn finding_of(
    run: &RunCaptureResult,
    directory: &Path,
    pipeline: &Pipeline,
    settings: &ProfileSettings,
) -> Finding {
    let profile = read_samples(directory)
        .map(|(record, table)| profile_report(&record, &table, pipeline, settings));
    match (died(run), profile) {
        (true, profile) => Finding::Died {
            diagnostics: death_diagnostics(run),
            profile: profile.ok(),
        },
        (false, Ok(profile)) => Finding::Profile(profile),
        (false, Err(diagnostic)) => Finding::NoProfile(diagnostic),
    }
}

/// The report of `record`, read against `table`.
fn profile_report(
    record: &ProfileRecord,
    table: &LineTable,
    pipeline: &Pipeline,
    settings: &ProfileSettings,
) -> JsonProfile {
    let shape = ReportShape {
        top: settings.top,
        hz: record.hz,
        dropped: record.dropped,
        truncated: record.truncated,
        files: &table.files,
    };
    let mut sources = SourceTexts::new(pipeline, &table.files);
    let attribution = Attribution::of(record, table);
    report::report(&attribution, &shape, |line| sources.line_text(line))
}

/// The record the run left in `directory` and the line map its build wrote,
/// or why there are no samples to report.
fn read_samples(directory: &Path) -> Result<(ProfileRecord, LineTable), Box<Diagnostic>> {
    let Ok(text) = std::fs::read_to_string(directory.join(RECORD_NAME)) else {
        return Err(no_profile(
            "the program finished without writing a profile record".to_string(),
        ));
    };
    let lines_path = directory.join(format!("{RUN_EXECUTABLE_NAME}.lines"));
    let (record, table) = ProfileRecord::parse(&text)
        .and_then(|record| {
            let map = std::fs::read_to_string(&lines_path).map_err(|error| error.to_string())?;
            Ok((record, LineTable::parse(&map)?))
        })
        .map_err(|reason| no_profile(format!("the profile record could not be read: {reason}")))?;
    if record.samples() == 0 {
        return Err(no_profile(
            "the program finished before the profiler took a sample".to_string(),
        ));
    }
    Ok((record, table))
}

/// The diagnostic for a run that left no samples, saying `why`.
fn no_profile(why: String) -> Box<Diagnostic> {
    coded(
        DiagnosticCode::BldNoProfileRecorded,
        why,
        "profile a run that spends more CPU time, with a larger input or a higher --hz; \
         the record is written when the program exits, so a program that ends some other \
         way leaves none",
    )
}

/// Why a run died: the profile code first, then the trap or signal, as
/// `miri run` reports it.
fn death_diagnostics(run: &RunCaptureResult) -> Vec<Diagnostic> {
    let (cause, reason) = match (run.signal, run.trap_code) {
        (Some(signal), _) => {
            let (code, _) = crate::diagnostics::signal::signal_to_code_and_name(signal);
            (
                Some(code),
                crate::diagnostics::signal::signal_message(signal),
            )
        }
        (None, Some(code)) => (Some(code), format!("it stopped on {}", code.as_str())),
        (None, None) => (None, "it ended without an exit status".to_string()),
    };
    let failed = coded(
        DiagnosticCode::BldProfiledProgramFailed,
        format!("the program died while it was profiled: {reason}"),
        "a profile is taken of a run that finishes; 'miri run' on the same file reports the \
         failure where it happens",
    );
    std::iter::once(*failed)
        .chain(cause.map(cause_diagnostic))
        .collect()
}

/// The diagnostic naming what a run died of, worded as the registry words
/// it.
fn cause_diagnostic(code: DiagnosticCode) -> Diagnostic {
    crate::error::diagnostic::DiagnosticBuilder::error(code.title().to_string())
        .code(code.as_str())
        .message(code.title().to_string())
        .build()
}

/// The file of a `local:` module named `name`, under `program_dir`. The
/// name comes from the line map, a file on disk, so one that is absolute or
/// climbs out of the directory with `..` names no module of the program and
/// is not read.
fn local_module_path(program_dir: &Path, name: &str) -> Option<PathBuf> {
    use std::path::Component;
    let relative = Path::new(name);
    let stays_inside = relative
        .components()
        .all(|component| matches!(component, Component::Normal(_) | Component::CurDir));
    stays_inside.then(|| program_dir.join(relative))
}

/// The text of a program's source files, read once each, as the line map
/// names them: the program, a module of its own (`local:`), or a library
/// module under the library roots.
struct SourceTexts<'p> {
    program: Option<PathBuf>,
    files: &'p [String],
    texts: HashMap<usize, Option<String>>,
}

impl<'p> SourceTexts<'p> {
    fn new(pipeline: &Pipeline, files: &'p [String]) -> Self {
        Self {
            program: pipeline.source_path().map(PathBuf::from),
            files,
            texts: HashMap::new(),
        }
    }

    /// The file the line map names at `index`, on this machine.
    fn locate(&self, index: usize) -> Option<PathBuf> {
        let program = self.program.as_ref()?;
        if index == 0 {
            return Some(program.clone());
        }
        let name = self.files.get(index)?;
        if let Some(local) = name.strip_prefix("local:") {
            return local_module_path(program.parent()?, local);
        }
        crate::type_checker::statements::imports::stdlib_roots()
            .into_iter()
            .map(|root| root.join(name))
            .find(|path| path.is_file())
    }

    /// The text of `line`, its line ending dropped.
    fn line_text(&mut self, line: SourceLine) -> Option<String> {
        if !self.texts.contains_key(&line.file) {
            let text = self
                .locate(line.file)
                .and_then(|path| std::fs::read_to_string(path).ok());
            self.texts.insert(line.file, text);
        }
        let text = self.texts.get(&line.file)?.as_deref()?;
        let found = text.split('\n').nth(line.line.checked_sub(1)?)?;
        Some(found.trim_end_matches('\r').to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_module_is_found_under_the_program_directory_only() {
        let program = Path::new("/work/app");
        assert_eq!(
            local_module_path(program, "util/strings.mi"),
            Some(PathBuf::from("/work/app/util/strings.mi"))
        );
        assert_eq!(local_module_path(program, "../secret.mi"), None);
        assert_eq!(local_module_path(program, "util/../../secret.mi"), None);
        assert_eq!(local_module_path(program, "/etc/passwd"), None);
    }
}

/// The exit status the command ends with for `verdict`.
pub fn exit_code_of(verdict: &Verdict) -> i32 {
    match verdict {
        Verdict::Unsupported(_) | Verdict::NotRun(_) => 1,
        Verdict::Ran { run, finding } => match finding {
            Finding::NoProfile(_) => 1,
            Finding::Profile(_) | Finding::Died { .. } => run_status(run),
        },
    }
}

/// The status a run ended with, 128 plus the signal for a signal death, and
/// 1 for a program that died with none.
fn run_status(run: &RunCaptureResult) -> i32 {
    match run.signal {
        Some(signal) => 128_i32.saturating_add(signal),
        None if run.exit_code == NO_EXIT_STATUS => 1,
        None => run.exit_code,
    }
}

/// The diagnostics `verdict` reports, before the notices compiling raised.
/// A program that does not compile is reported against its file, as `miri
/// run` reports it; the command's own diagnostics name no file.
fn verdict_diagnostics(
    verdict: &Verdict,
    source: &str,
    source_path: Option<&str>,
) -> Vec<JsonDiagnostic> {
    let coded_json = |diagnostic: &Diagnostic| to_json(diagnostic, source, None);
    match verdict {
        Verdict::Unsupported(diagnostic) => vec![coded_json(diagnostic)],
        Verdict::NotRun(error) => error
            .to_diagnostics()
            .iter()
            .map(|diagnostic| to_json(diagnostic, source, source_path))
            .collect(),
        Verdict::Ran { finding, .. } => match finding {
            Finding::Profile(_) => Vec::new(),
            Finding::NoProfile(diagnostic) => vec![coded_json(diagnostic)],
            Finding::Died { diagnostics, .. } => diagnostics.iter().map(coded_json).collect(),
        },
    }
}

/// `envelope` carrying the tails of what `run` printed, and its signal.
fn with_run_output(envelope: DiagnosticsEnvelope, run: &RunCaptureResult) -> DiagnosticsEnvelope {
    let (stdout, stdout_truncated) = output_tail(&run.stdout, OUTPUT_TAIL_BYTES);
    let (stderr, stderr_truncated) = output_tail(&run.stderr, OUTPUT_TAIL_BYTES);
    let envelope = envelope
        .with_stdout(stdout, stdout_truncated)
        .with_stderr(stderr, stderr_truncated);
    match run.signal {
        Some(signal) => envelope.with_signal(signal),
        None => envelope,
    }
}

/// The envelope `--format json` prints for `profiled`. `ok` is true exactly
/// when the run finished and left samples to report.
pub fn envelope(profiled: &Profiled, source: &str) -> DiagnosticsEnvelope {
    let source_path = profiled.pipeline.source_path();
    let notices = profiled
        .notices
        .iter()
        .map(|notice| to_json(notice, source, source_path));
    let diagnostics = verdict_diagnostics(&profiled.verdict, source, source_path)
        .into_iter()
        .chain(notices)
        .collect();
    let (run, finding) = match &profiled.verdict {
        Verdict::Ran { run, finding } => (Some(run), Some(finding)),
        Verdict::Unsupported(_) | Verdict::NotRun(_) => (None, None),
    };
    let ok = matches!(finding, Some(Finding::Profile(_)));
    let profile = finding.and_then(Finding::profile);
    let mut envelope = DiagnosticsEnvelope::new(JsonCommand::Profile, ok, diagnostics)
        .with_exit_code(exit_code_of(&profiled.verdict))
        .with_duration_ms(profiled.elapsed_ms);
    if let Some(run) = run {
        envelope = with_run_output(envelope, run);
    }
    match profile {
        Some(profile) => envelope.with_profile(profile.clone()),
        None => envelope,
    }
}

/// Write `profiled` as a person reads it: the program's own output as it
/// printed it, then the report or what went wrong, on stderr.
fn write_pretty(profiled: &Profiled, source: &str, color: ColorMode) {
    let source_path = profiled.pipeline.source_path();
    profiled.pipeline.print_notices(source, &profiled.notices);
    let print = |diagnostic: &Diagnostic, path: Option<&str>| {
        eprintln!(
            "{}",
            format_diagnostic_with_color(source, diagnostic, path, color.into())
        );
    };
    match &profiled.verdict {
        Verdict::Unsupported(diagnostic) => print(diagnostic, None),
        Verdict::NotRun(error) => eprintln!(
            "{}",
            error.report_with_path_and_color(source, source_path, color.into())
        ),
        Verdict::Ran { run, finding } => {
            print!("{}", String::from_utf8_lossy(&run.stdout));
            eprint!("{}", String::from_utf8_lossy(&run.stderr));
            match finding {
                Finding::Profile(profile) => eprint!("{}", report::render(profile)),
                Finding::NoProfile(diagnostic) => print(diagnostic, None),
                Finding::Died {
                    diagnostics,
                    profile,
                } => {
                    if let Some(profile) = profile {
                        eprint!("{}", report::render(profile));
                    }
                    diagnostics.iter().for_each(|d| print(d, None));
                }
            }
        }
    }
}

/// Profile the requested file and write the result. Returns the status the
/// command ends with: the program's own when it was profiled, 1 when it
/// could not be.
pub fn run(request: &ProfileRequest<'_>, format: Format, color: ColorMode) -> i32 {
    let Some(source) =
        crate::cli::source::read_or_report(request.path, JsonCommand::Profile, format, color)
    else {
        return 1;
    };
    let profiled = profile(request, &source);
    match format {
        Format::Json => println!("{}", serialize_envelope(&envelope(&profiled, &source))),
        Format::Pretty => write_pretty(&profiled, &source, color),
    }
    exit_code_of(&profiled.verdict)
}
