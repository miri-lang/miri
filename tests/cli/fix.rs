// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Behaviour of `miri fix`.
//!
//! A repair is only worth offering if applying it leaves a program the compiler
//! accepts, so most of these tests apply the edits and re-check the result
//! rather than asserting on the shape of the plan.

use crate::utils::miri_cmd;
use miri::diagnostics::json::{DiagnosticsEnvelope, JsonCommand, JsonDiagnostic};
use std::fs;
use std::path::{Path, PathBuf};

/// A source file in a directory of its own, removed when the test ends.
struct Fixture {
    directory: PathBuf,
    file: PathBuf,
}

impl Fixture {
    fn new(name: &str, source: &str) -> Self {
        let directory = std::env::temp_dir().join(format!("miri-fix-{}", name));
        let _ = fs::remove_dir_all(&directory);
        fs::create_dir_all(&directory).expect("could not create the fixture directory");
        let file = directory.join("main.mi");
        fs::write(&file, source).expect("could not write the fixture source");
        Self { directory, file }
    }

    fn path(&self) -> &Path {
        &self.file
    }

    fn contents(&self) -> String {
        fs::read_to_string(&self.file).expect("could not read the fixture source")
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

/// Run `miri fix` with `args` against `path` and return (stdout, stderr, success).
fn fix(path: &Path, args: &[&str]) -> (String, String, bool) {
    let output = miri_cmd()
        .arg("fix")
        .args(args)
        .arg(path)
        .output()
        .expect("failed to run the fix command");
    (
        String::from_utf8_lossy(&output.stdout).into_owned(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
        output.status.success(),
    )
}

/// The plan `miri fix` reports for `path`, as a parsed envelope.
fn plan(path: &Path) -> DiagnosticsEnvelope {
    let (stdout, _, _) = fix(path, &["--format", "json"]);
    serde_json::from_str(&stdout).expect("fix did not emit a parseable envelope")
}

/// Whether `miri check` accepts `path`.
fn checks_clean(path: &Path) -> bool {
    miri_cmd()
        .arg("check")
        .arg(path)
        .output()
        .expect("failed to run the check command")
        .status
        .success()
}

fn repair_ids(envelope: &DiagnosticsEnvelope) -> Vec<String> {
    envelope
        .diagnostics
        .iter()
        .filter_map(|diagnostic| diagnostic.repair.as_ref())
        .map(|repair| repair.id.clone())
        .collect()
}

fn diagnostic_with_code<'a>(envelope: &'a DiagnosticsEnvelope, code: &str) -> &'a JsonDiagnostic {
    envelope
        .diagnostics
        .iter()
        .find(|diagnostic| diagnostic.code.as_deref() == Some(code))
        .unwrap_or_else(|| panic!("no diagnostic carried {}", code))
}

#[test]
fn test_plan_reports_the_fix_command_in_a_schema_version_one_envelope() {
    let fixture = Fixture::new("envelope", "fn main()\n    let x = 1\n    x = 2\n");
    let envelope = plan(fixture.path());

    assert_eq!(envelope.schema_version, 1);
    assert_eq!(envelope.command, JsonCommand::Fix);
    assert!(
        !envelope.diagnostics.is_empty(),
        "the program has an error, so the envelope should carry it"
    );
    assert_eq!(
        repair_ids(&envelope),
        vec!["let-to-var"],
        "the envelope should carry the repair, not just the diagnostic"
    );
}

#[test]
fn test_plan_leaves_the_source_untouched() {
    let source = "fn main()\n    let x = 1\n    x = 2\n";
    let fixture = Fixture::new("plan-is-read-only", source);

    let envelope = plan(fixture.path());

    assert!(
        !repair_ids(&envelope).is_empty(),
        "expected a repair to plan"
    );
    assert_eq!(
        fixture.contents(),
        source,
        "planning must not write to the file"
    );
}

#[test]
fn test_reassigned_let_is_repaired_into_a_var_that_checks() {
    let fixture = Fixture::new("let-to-var", "fn main()\n    let x = 1\n    x = 2\n");

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["let-to-var"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().contains("var x = 1"),
        "the declaration should now read `var`, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_a_shared_declaration_keyword_is_never_rewritten() {
    // `let a = 1, b = 2` binds both names through one keyword, so rewriting it
    // would make `b` mutable as well as `a`. No repair is the correct answer.
    let fixture = Fixture::new(
        "shared-keyword",
        "fn main()\n    let a = 1, b = 2\n    a = 3\n    println(f\"{a}{b}\")\n",
    );

    let envelope = plan(fixture.path());

    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_042");
    assert!(
        diagnostic.repair.is_none(),
        "a keyword shared between bindings must not be repaired, got {:?}",
        diagnostic.repair
    );
}

#[test]
fn test_a_reassigned_constant_is_not_repaired_into_a_var() {
    // A constant is a different declaration form; rewriting `const` as `var`
    // would not compile, so the condition is reported without a repair.
    let fixture = Fixture::new("constant", "fn main()\n    const K = 1\n    K = 2\n");

    let envelope = plan(fixture.path());

    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_042");
    assert!(
        diagnostic.repair.is_none(),
        "a constant must not be repaired into a var, got {:?}",
        diagnostic.repair
    );
}

#[test]
fn test_an_unimported_name_is_repaired_by_importing_its_module() {
    let fixture = Fixture::new(
        "add-import",
        "fn main()\n    let r = sqrt(4.0)\n    println(f\"{r}\")\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["add-import"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().starts_with("use system.math.{sqrt}"),
        "the import should lead the file, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_an_import_lands_after_the_imports_a_file_already_has() {
    let fixture = Fixture::new(
        "import-placement",
        "use system.io.{println}\n\nfn main()\n    let r = sqrt(4.0)\n    println(f\"{r}\")\n",
    );

    assert_eq!(
        repair_ids(&plan(fixture.path())),
        vec!["add-import"],
        "the planner should choose the import repair"
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);

    assert!(ok, "applying the repair should succeed");
    let contents = fixture.contents();
    let existing = contents
        .find("use system.io")
        .expect("the original import should survive");
    let added = contents
        .find("use system.math")
        .expect("the new import should be present");
    assert!(
        added > existing,
        "the new import should follow the existing one, got: {}",
        contents
    );
    assert!(checks_clean(fixture.path()));
}

#[test]
fn test_surplus_arguments_are_dropped_leaving_the_declared_ones() {
    let fixture = Fixture::new(
        "drop-extra-arguments",
        "fn add(a int, b int) int\n    return a + b\n\nfn main()\n    println(f\"{add(1, 2, 3)}\")\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["drop-extra-arguments"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().contains("add(1, 2)"),
        "the declared arguments should remain, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_dropping_every_argument_leaves_the_parentheses_intact() {
    let fixture = Fixture::new(
        "drop-all-arguments",
        "fn greet()\n    println(\"hi\")\n\nfn main()\n    greet(1, 2)\n",
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);

    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().contains("greet()"),
        "the call should keep its parentheses, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_apply_without_confirmation_refuses_and_writes_nothing() {
    let source = "fn main()\n    let x = 1\n    x = 2\n";
    let fixture = Fixture::new("apply-unconfirmed", source);

    let (_, stderr, ok) = fix(fixture.path(), &["--apply"]);

    assert!(!ok, "applying without --yes should exit non-zero");
    assert!(
        stderr.contains("--yes"),
        "the refusal should name the flag that confirms, got: {}",
        stderr
    );
    assert_eq!(
        fixture.contents(),
        source,
        "a refused apply must leave the file byte-identical"
    );
}

#[test]
fn test_a_program_with_nothing_to_repair_reports_no_repairs() {
    let source = "fn main()\n    let x = 1\n    println(f\"{x}\")\n";
    let fixture = Fixture::new("nothing-to-repair", source);

    let envelope = plan(fixture.path());

    assert!(envelope.ok, "a clean program should report ok");
    assert!(repair_ids(&envelope).is_empty());
    assert_eq!(fixture.contents(), source);
}

#[test]
fn test_every_planned_edit_names_a_range_inside_the_file() {
    let fixture = Fixture::new(
        "edit-ranges",
        "fn add(a int, b int) int\n    return a + b\n\nfn main()\n    println(f\"{add(1, 2, 3)}\")\n",
    );
    let source = fixture.contents();

    let envelope = plan(fixture.path());

    let mut seen = 0;
    for diagnostic in &envelope.diagnostics {
        let Some(repair) = &diagnostic.repair else {
            continue;
        };
        assert!(!repair.edits.is_empty(), "a repair must carry edits");
        for edit in &repair.edits {
            seen += 1;
            assert!(
                edit.start <= edit.end,
                "an edit range must not run backwards"
            );
            assert!(
                edit.end <= source.len(),
                "an edit must stay inside the file"
            );
            assert!(source.is_char_boundary(edit.start));
            assert!(source.is_char_boundary(edit.end));
        }
    }
    assert!(seen > 0, "expected at least one edit to inspect");
}

/// A standard library laid out under a directory of its own.
///
/// Building one lets a test state exactly which modules declare a name, which
/// is the only way to exercise the ambiguity rule without depending on what the
/// real standard library happens to contain today.
struct TemporaryStdlib {
    root: PathBuf,
}

impl TemporaryStdlib {
    fn new(name: &str, modules: &[(&str, &str)]) -> Self {
        let root = std::env::temp_dir().join(format!("miri-fix-stdlib-{}", name));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("system")).expect("could not create the stdlib directory");
        for (module, source) in modules {
            fs::write(root.join("system").join(format!("{}.mi", module)), source)
                .expect("could not write a stdlib module");
        }
        Self { root }
    }

    /// The repair planned for `name` in `source`, if any.
    fn plan_repair_for(&self, source_path: &Path, name: &str) -> Option<String> {
        let output = miri_cmd()
            .env("MIRI_STDLIB_PATH", &self.root)
            .arg("fix")
            .arg("--format")
            .arg("json")
            .arg(source_path)
            .output()
            .expect("failed to run the fix command");
        let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
        let envelope: DiagnosticsEnvelope =
            serde_json::from_str(&stdout).expect("fix did not emit a parseable envelope");
        envelope
            .diagnostics
            .iter()
            .find(|diagnostic| diagnostic.message.ends_with(name))
            .unwrap_or_else(|| panic!("no diagnostic named {}", name))
            .repair
            .as_ref()
            .map(|repair| repair.summary.clone())
    }
}

impl Drop for TemporaryStdlib {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn test_a_name_declared_by_one_module_is_repaired_by_importing_it() {
    let stdlib = TemporaryStdlib::new(
        "unambiguous",
        &[("alpha", "fn twice(v int) int\n    return v * 2\n")],
    );
    let fixture = Fixture::new("import-unambiguous", "fn main()\n    twice(2)\n");

    let repair = stdlib.plan_repair_for(fixture.path(), "twice");

    assert_eq!(
        repair.as_deref(),
        Some("Import `twice` from `system.alpha`."),
        "a name declared by exactly one module should be importable"
    );
}

#[test]
fn test_a_name_declared_by_two_modules_is_not_repaired() {
    // Choosing between the candidates is the author's decision, and this repair
    // can be written straight to disk. Guessing is not an option.
    let stdlib = TemporaryStdlib::new(
        "ambiguous",
        &[
            ("alpha", "fn twice(v int) int\n    return v * 2\n"),
            ("beta", "fn twice(v int) int\n    return v + v\n"),
        ],
    );
    let fixture = Fixture::new("import-ambiguous", "fn main()\n    twice(2)\n");

    let repair = stdlib.plan_repair_for(fixture.path(), "twice");

    assert!(
        repair.is_none(),
        "an ambiguous name must not be repaired, got {:?}",
        repair
    );
}

#[test]
fn test_offsets_survive_multi_byte_characters_before_the_edit() {
    // Edits are byte offsets. Text above the edit site that is wider than one
    // byte per character is what would expose an offset counted in characters.
    let fixture = Fixture::new(
        "multi-byte",
        "// ha\u{ff}ndler \u{1f600} caf\u{e9}\nfn main()\n    let x = 1\n    x = 2\n",
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);

    assert!(ok, "applying the repair should succeed");
    let contents = fixture.contents();
    assert!(
        contents.contains("var x = 1"),
        "the declaration should be the text rewritten, got: {}",
        contents
    );
    assert!(
        contents.contains('\u{1f600}'),
        "the text above the edit should survive intact, got: {}",
        contents
    );
    assert!(checks_clean(fixture.path()));
}

#[test]
fn test_several_repairs_in_one_file_are_applied_together() {
    let fixture = Fixture::new(
        "several-repairs",
        "fn add(a int, b int) int\n    return a + b\n\nfn main()\n    let x = 1\n    x = 2\n    println(f\"{add(x, 2, 3)}\")\n",
    );

    let mut ids = repair_ids(&plan(fixture.path()));
    ids.sort();
    assert_eq!(ids, vec!["drop-extra-arguments", "let-to-var"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);

    assert!(ok, "applying both repairs should succeed");
    let contents = fixture.contents();
    assert!(contents.contains("var x = 1"), "got: {}", contents);
    assert!(contents.contains("add(x, 2)"), "got: {}", contents);
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_applying_plans_against_the_file_as_it_stands() {
    // Planning and applying happen in one invocation, so a file edited since an
    // earlier run is re-read and re-planned rather than repaired from a stale
    // offset. Here the edited text no longer parses, so there is nothing to do.
    let fixture = Fixture::new("replanned", "fn main()\n    let x = 1\n    x = 2\n");
    assert_eq!(repair_ids(&plan(fixture.path())), vec!["let-to-var"]);

    let replaced = "fn main()\n    LET x = 1\n    x = 2\n";
    fs::write(fixture.path(), replaced).expect("could not rewrite the fixture");

    let (_, _, _) = fix(fixture.path(), &["--apply", "--yes"]);

    assert_eq!(
        fixture.contents(),
        replaced,
        "text with no repair to offer must be left alone"
    );
}

#[test]
fn test_a_repair_belonging_to_an_imported_file_is_not_applied() {
    // The caller named one file. A diagnostic raised inside a file it imports
    // carries that file's path, and rewriting it would edit a file the caller
    // never asked about.
    let directory = std::env::temp_dir().join("miri-fix-imported");
    let _ = fs::remove_dir_all(&directory);
    fs::create_dir_all(directory.join("lib")).expect("could not create the fixture tree");
    let imported = directory.join("lib").join("helper.mi");
    let imported_source = "fn twice(x int) int\n    let acc = x\n    acc = x + x\n    return acc\n";
    fs::write(&imported, imported_source).expect("could not write the imported module");
    let main = directory.join("main.mi");
    fs::write(
        &main,
        "use local.lib.helper\n\nfn main()\n    println(f\"{twice(3)}\")\n",
    )
    .expect("could not write the main module");

    let (_, stderr, _) = fix(&main, &["--apply", "--yes"]);

    assert_eq!(
        fs::read_to_string(&imported).expect("the imported module should still exist"),
        imported_source,
        "a file the caller did not name must not be rewritten"
    );
    assert!(
        stderr.contains("skipped"),
        "the skipped repair should be reported, got: {}",
        stderr
    );
    let _ = fs::remove_dir_all(&directory);
}

#[test]
fn test_unimported_list_collection_is_repaired_with_import_and_checks_clean() {
    let fixture = Fixture::new("unimported-list", "fn main()\n    let l = List([1, 2])\n");

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["add-import"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture
            .contents()
            .starts_with("use system.collections.list"),
        "the import should lead the file, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_unimported_map_collection_is_repaired_with_import_and_checks_clean() {
    let fixture = Fixture::new(
        "unimported-map",
        "fn main()\n    let m = Map<String, int>()\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["add-import"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().starts_with("use system.collections.map"),
        "the import should lead the file, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_unimported_set_collection_is_repaired_with_import_and_checks_clean() {
    let fixture = Fixture::new("unimported-set", "fn main()\n    let s = Set({1})\n");

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["add-import"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().starts_with("use system.collections.set"),
        "the import should lead the file, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_unimported_json_is_repaired_with_import_and_checks_clean() {
    let fixture = Fixture::new(
        "unimported-json",
        "fn main()\n    let j = Json.parse(\"{}\")\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["add-import"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().starts_with("use system.json"),
        "the import should lead the file, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_ambiguous_unimported_type_gets_no_repair() {
    let stdlib = TemporaryStdlib::new(
        "ambiguous-widget",
        &[("alpha", "class Widget\n"), ("beta", "class Widget\n")],
    );
    let fixture = Fixture::new("ambiguous-widget", "fn main()\n    let w = Widget()\n");

    let repair = stdlib.plan_repair_for(fixture.path(), "Widget");
    assert!(
        repair.is_none(),
        "type declared in two modules should not get a repair, got {:?}",
        repair
    );
    assert!(
        !checks_clean(fixture.path()),
        "an unimported ambiguous type should still be an error, repair or not"
    );
}

/// A type is found however it is declared, not only when it is a `class`.
///
/// The stdlib happens to declare its types as classes and enums, so a scan that
/// silently missed the other forms would still pass every test written against
/// it. These modules name one type per declaration form instead.
#[test]
fn test_a_type_is_repaired_whichever_form_declares_it() {
    let forms = [
        ("astruct", "struct Marker\n    x int\n", "Marker"),
        ("atrait", "trait Marker\n    fn ping()\n", "Marker"),
        ("analias", "type Marker is String\n", "Marker"),
    ];

    for (module, source, name) in forms {
        let stdlib = TemporaryStdlib::new(module, &[(module, source)]);
        let fixture = Fixture::new(
            &format!("declared-as-{}", module),
            "fn main()\n    let v = Marker()\n",
        );

        assert_eq!(
            stdlib.plan_repair_for(fixture.path(), name).as_deref(),
            Some(format!("Import `Marker` from `system.{}`.", module).as_str()),
            "a type declared in {} should be importable",
            source.trim()
        );
    }
}

#[test]
fn test_selective_import_hides_sibling_types() {
    let fixture = Fixture::new(
        "selective-import-hides-sibling",
        "use system.json.{Json}\n\nfn main()\n    let e = JsonError.TrailingData(1, 1)\n",
    );
    assert!(
        !checks_clean(fixture.path()),
        "JsonError should not be directly accessible with selective import"
    );
}

#[test]
fn test_selective_import_of_json_checks_clean() {
    let fixture = Fixture::new(
        "selective-import-json",
        "use system.json.{Json}\n\nfn main()\n    let j = Json.parse(\"{}\")\n",
    );
    assert!(
        checks_clean(fixture.path()),
        "Json imported selectively should be usable and check clean"
    );
}
