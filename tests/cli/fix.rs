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

/// Applies the one repair `source` is expected to offer, and returns the text
/// it leaves behind.
///
/// Every foreign-syntax repair makes the same promise: the file it writes is one
/// the compiler accepts. Asserting the text alone would let a repair that
/// produces plausible-looking but unparseable source pass, so the check is part
/// of the helper rather than left to each caller to remember.
fn repaired(name: &str, source: &str, expected_id: &str) -> String {
    let fixture = Fixture::new(name, source);

    let envelope = plan(fixture.path());
    assert_eq!(
        repair_ids(&envelope),
        vec![expected_id.to_string()],
        "the plan should offer exactly the {} repair",
        expected_id
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the {} repair should succeed", expected_id);
    assert!(
        checks_clean(fixture.path()),
        "the source {} left behind should check clean, got: {}",
        expected_id,
        fixture.contents()
    );

    fixture.contents()
}

#[test]
fn test_a_colon_type_annotation_loses_its_colon() {
    assert_eq!(
        repaired(
            "colon-annotation",
            "fn main()\n    let x: int = 5\n    println(\"{x}\")\n",
            "colon-annotation",
        ),
        "fn main()\n    let x int = 5\n    println(\"{x}\")\n"
    );
}

#[test]
fn test_a_colon_annotation_without_a_space_gains_one() {
    // Deleting the colon alone would run the name into the type, so the repair
    // writes back the separator Miri needs rather than removing bytes.
    assert_eq!(
        repaired(
            "colon-annotation-tight",
            "fn main()\n    let x:int = 5\n    println(\"{x}\")\n",
            "colon-annotation",
        ),
        "fn main()\n    let x int = 5\n    println(\"{x}\")\n"
    );
}

#[test]
fn test_an_arrow_return_type_loses_its_arrow() {
    assert_eq!(
        repaired(
            "arrow-return-type",
            "fn main() -> int\n    return 0\n",
            "arrow-return-type",
        ),
        "fn main() int\n    return 0\n"
    );
}

#[test]
fn test_an_arrow_return_type_without_spaces_gains_one() {
    assert_eq!(
        repaired(
            "arrow-return-type-tight",
            "fn main()->int\n    return 0\n",
            "arrow-return-type",
        ),
        "fn main() int\n    return 0\n"
    );
}

#[test]
fn test_a_let_mut_binding_becomes_a_var() {
    assert_eq!(
        repaired(
            "let-mut-to-var",
            "fn main()\n    let mut x = 5\n    x = 6\n    println(\"{x}\")\n",
            "let-mut-to-var",
        ),
        "fn main()\n    var x = 5\n    x = 6\n    println(\"{x}\")\n"
    );
}

#[test]
fn test_a_let_mut_binding_spaced_out_still_becomes_a_var() {
    // The repair spans from the keyword to the end of `mut`, so it does not
    // depend on the two words being one space apart.
    assert_eq!(
        repaired(
            "let-mut-to-var-spaced",
            "fn main()\n    let   mut x = 5\n    x = 6\n",
            "let-mut-to-var",
        ),
        "fn main()\n    var x = 5\n    x = 6\n"
    );
}

#[test]
fn test_a_let_mut_binding_with_a_comment_between_the_words_is_not_repaired() {
    // The edit replaces the whole `let mut` span, so anything between the two
    // words would be deleted with them. A repair that cannot be made without
    // discarding the author's text is not offered at all.
    let fixture = Fixture::new(
        "let-mut-to-var-commented",
        "fn main()\n    let /*keep me*/ mut x = 5\n    x = 6\n",
    );

    let envelope = plan(fixture.path());
    // The construct is still recognised — only the edit is withheld. Asserting
    // the help as well is what keeps this from passing if the diagnostic were to
    // stop being raised at all.
    let diagnostic = diagnostic_with_code(&envelope, "MER_PAR_001");
    assert_eq!(
        diagnostic.help.as_deref(),
        Some("Miri declares a mutable binding with `var`: `var x = 5`.")
    );
    assert!(
        repair_ids(&envelope).is_empty(),
        "a `let mut` split by a comment should offer no repair, got: {:?}",
        repair_ids(&envelope)
    );
}

#[test]
fn test_a_macro_style_call_loses_its_bang() {
    assert_eq!(
        repaired(
            "println-bang",
            "fn main()\n    println!(\"hi\")\n",
            "println-bang",
        ),
        "fn main()\n    println(\"hi\")\n"
    );
}

#[test]
fn test_a_bang_negation_becomes_not() {
    assert_eq!(
        repaired(
            "word-operator-not",
            "fn main()\n    let s = \"a\"\n    if !s.is_empty()\n        println(s)\n",
            "word-operator",
        ),
        "fn main()\n    let s = \"a\"\n    if not s.is_empty()\n        println(s)\n"
    );
}

#[test]
fn test_a_double_ampersand_becomes_and() {
    assert_eq!(
        repaired(
            "word-operator-and",
            "fn main()\n    let a = true\n    if a && a\n        println(\"y\")\n",
            "word-operator",
        ),
        "fn main()\n    let a = true\n    if a and a\n        println(\"y\")\n"
    );
}

#[test]
fn test_a_tight_double_pipe_becomes_a_spaced_or() {
    // Writing the word in place of the symbols alone would run it into its
    // operands, so the repair supplies the spaces the symbols did not need.
    assert_eq!(
        repaired(
            "word-operator-or",
            "fn main()\n    let a = true\n    if a||a\n        println(\"y\")\n",
            "word-operator",
        ),
        "fn main()\n    let a = true\n    if a or a\n        println(\"y\")\n"
    );
}

#[test]
fn test_a_then_after_an_if_condition_becomes_a_colon() {
    assert_eq!(
        repaired(
            "if-colon-then",
            "fn main()\n    let d = true\n    if d then\n        println(\"y\")\n",
            "if-colon",
        ),
        "fn main()\n    let d = true\n    if d:\n        println(\"y\")\n"
    );
}

#[test]
fn test_an_inline_else_gains_its_colon() {
    assert_eq!(
        repaired(
            "if-colon-else",
            "fn main()\n    let d = true\n    let m = if d: \"x\" else \"-\"\n    println(m)\n",
            "if-colon",
        ),
        "fn main()\n    let d = true\n    let m = if d: \"x\" else: \"-\"\n    println(m)\n"
    );
}

#[test]
fn test_an_inline_if_then_else_is_repaired_in_two_passes() {
    let fixture = Fixture::new(
        "if-colon-then-else",
        "fn main()\n    let d = true\n    let m = if d then \"x\" else \"-\"\n    println(m)\n",
    );
    for _ in 0..2 {
        let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
        assert!(ok, "each pass should apply the repair it plans");
    }
    assert_eq!(
        fixture.contents(),
        "fn main()\n    let d = true\n    let m = if d: \"x\" else: \"-\"\n    println(m)\n"
    );
    assert!(checks_clean(fixture.path()));
}

#[test]
fn test_quotes_nested_in_an_interpolation_take_the_other_quote() {
    assert_eq!(
        repaired(
            "interpolation-quotes",
            "fn main()\n    println(f\"{\"a\".compare(\"b\")} done\")\n",
            "interpolation-quotes",
        ),
        "fn main()\n    println(f\"{'a'.compare('b')} done\")\n"
    );
}

#[test]
fn test_an_interpolation_already_holding_both_quotes_is_not_repaired() {
    let fixture = Fixture::new(
        "interpolation-quotes-mixed",
        "fn main()\n    println(f\"{\"it's\".length()}\")\n",
    );
    let envelope = plan(fixture.path());
    assert!(
        repair_ids(&envelope).is_empty(),
        "swapping the quotes would collide with the apostrophe: {:?}",
        repair_ids(&envelope)
    );
}

#[test]
fn test_a_null_literal_becomes_none() {
    assert_eq!(
        repaired(
            "null-to-none",
            "fn f() String?\n    return null\n\nfn main()\n    println(\"x\")\n",
            "null-to-none",
        ),
        "fn f() String?\n    return None\n\nfn main()\n    println(\"x\")\n"
    );
}

#[test]
fn test_a_foreign_form_whose_rewrite_is_structural_offers_no_repair() {
    // An indented block, a class body and a second binding are shapes rather
    // than substitutions. Offering a repair for them would mean writing code the
    // author has not written, so these carry help and nothing else.
    for (name, source) in [
        (
            "brace-block",
            "fn main()\n    if true {\n        println(\"x\")\n",
        ),
        (
            "elif",
            "fn main()\n    println(\"a\")\nelif true\n    println(\"b\")\n",
        ),
        (
            "impl-block",
            "class Foo\n    fn bar()\n        println(\"x\")\nimpl Foo\n    fn qux()\n        println(\"y\")\n",
        ),
        (
            "tuple-for-binding",
            "use system.collections.map\n\nfn main()\n    let m = Map<int, int>({})\n    for (k, v) in m\n        println(k)\n",
        ),
        (
            "destructuring-let",
            "fn main()\n    let (a, b) = (1, 2)\n    println(a)\n",
        ),
    ] {
        let fixture = Fixture::new(name, source);
        let envelope = plan(fixture.path());
        assert!(
            repair_ids(&envelope).is_empty(),
            "{} should carry help but no repair, got: {:?}",
            name,
            repair_ids(&envelope)
        );
    }
}

#[test]
fn test_foreign_forms_are_repaired_one_pass_at_a_time_until_the_file_checks() {
    // A syntax error stops the parse, so a file holding two foreign constructs
    // reveals the second only once the first is gone. The loop the skill
    // documents — check, fix, re-check — is what carries such a file to clean,
    // and this pins that it terminates rather than stalling on the first edit.
    let fixture = Fixture::new(
        "two-foreign-forms",
        "fn main()\n    let x: int = 5\n    let mut y = 6\n    y = 7\n    println(\"{x}\")\n",
    );

    let (_, _, first) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(first, "the first repair should apply");
    assert!(
        !checks_clean(fixture.path()),
        "the second construct should still be waiting"
    );

    let (_, _, second) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(second, "the second repair should apply");

    assert_eq!(
        fixture.contents(),
        "fn main()\n    let x int = 5\n    var y = 6\n    y = 7\n    println(\"{x}\")\n"
    );
    assert!(checks_clean(fixture.path()));
}

#[test]
fn test_the_miri_spellings_of_the_recognised_constructs_are_left_alone() {
    // Each recognition keys on something that is legal Miri elsewhere: `{` opens
    // a set or map, `(` groups an expression, `:` heads a single-line block or a
    // map entry, and `!=` carries a bang. A recognition firing on these would
    // reject correct programs, so the guard is that they still compile.
    for (name, source) in [
        (
            "set-literal",
            "use system.collections.set\n\nfn main()\n    let s = Set({1, 2})\n",
        ),
        (
            "map-literal",
            "use system.collections.map\n\nfn main()\n    let m = Map({\"a\": 1})\n",
        ),
        ("grouping", "fn main()\n    let x = (1 + 2)\n    println(\"{x}\")\n"),
        (
            "not-equal",
            "fn main()\n    let a = 1\n    if a != 2: println(\"ok\")\n",
        ),
        ("single-line-block", "fn double(x int) int:\n    x * 2\n"),
        (
            "match-arms",
            "fn main()\n    let x int? = 5\n    match x\n        Some(v): println(\"got\")\n        None: println(\"none\")\n",
        ),
        (
            "foreign-text-inside-a-string",
            "fn main()\n    let s = \"let mut x: int = println!\"\n    println(s)\n",
        ),
        (
            "binding-named-mut",
            "fn main()\n    let mut = 5\n    println(\"{mut}\")\n",
        ),
    ] {
        let fixture = Fixture::new(name, source);
        assert!(
            checks_clean(fixture.path()),
            "{} is legal Miri and must not be taken for a foreign construct",
            name
        );
    }
}

/// An apply that was asked to write repairs and wrote none, because the errors
/// in the file carry none, is a failure. Reporting it as success would tell a
/// caller the file was repaired when it was not.
#[test]
fn test_apply_with_no_repair_for_an_error_exits_non_zero() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.mi");
    std::fs::write(&path, "fn main()\n    undefined_thing()\n").unwrap();

    let output = miri_cmd()
        .arg("fix")
        .arg(&path)
        .arg("--apply")
        .arg("--yes")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(output.stdout).unwrap())
            .expect("stdout should be valid JSON");

    assert_eq!(parsed["ok"], false);
    assert_eq!(parsed["exitCode"], 1);
    assert_eq!(output.status.code(), Some(1));
    let codes: Vec<&str> = parsed["diagnostics"]
        .as_array()
        .expect("a list")
        .iter()
        .filter_map(|d| d["code"].as_str())
        .collect();
    assert!(
        codes.contains(&"MER_BLD_020"),
        "the refusal names itself: {:?}",
        codes
    );
}

/// A file with nothing wrong with it had nothing to repair, so an apply that
/// writes nothing did exactly what was asked and succeeds.
#[test]
fn test_apply_on_a_clean_file_succeeds() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.mi");
    std::fs::write(&path, "fn main()\n    println(\"hi\")\n").unwrap();

    let output = miri_cmd()
        .arg("fix")
        .arg(&path)
        .arg("--apply")
        .arg("--yes")
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(output.stdout).unwrap())
            .expect("stdout should be valid JSON");

    assert_eq!(parsed["ok"], true);
    assert_eq!(parsed["exitCode"], 0);
    assert_eq!(output.status.code(), Some(0));
}

/// A plan is reported as the success it is. Whether the file compiles is a
/// different question, answered by the diagnostics the plan was built from.
#[test]
fn test_plan_for_a_broken_file_reports_the_plan_as_produced() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("main.mi");
    std::fs::write(&path, "fn main()\n    undefined_thing()\n").unwrap();

    let output = miri_cmd()
        .arg("fix")
        .arg(&path)
        .arg("--format")
        .arg("json")
        .output()
        .unwrap();

    let parsed: serde_json::Value =
        serde_json::from_str(&String::from_utf8(output.stdout).unwrap())
            .expect("stdout should be valid JSON");

    assert_eq!(parsed["ok"], true, "the plan was produced");
    assert_eq!(parsed["exitCode"], 0);
    assert_eq!(output.status.code(), Some(0));
    assert!(
        !parsed["diagnostics"].as_array().expect("a list").is_empty(),
        "the file's errors still travel in the envelope"
    );
}

#[test]
fn test_a_key_accessor_on_a_mapping_is_repaired_into_iterating_it() {
    let fixture = Fixture::new(
        "drop-iterator-accessor",
        "use system.collections.map.{Map}\n\nfn main()\n    let tags = Map({\"a\": \"b\"})\n    \
         for k in tags.keys()\n        println(k)\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["drop-iterator-accessor"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().contains("for k in tags\n"),
        "the accessor and its parentheses should be gone, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_a_key_accessor_on_a_sequence_is_not_repaired() {
    // Iterating a list yields its elements, not its positions, so deleting
    // `keys` would change what the loop binds. The help still points at a `for`
    // loop; only the automatic edit is withheld.
    let fixture = Fixture::new(
        "keys-on-a-sequence",
        "use system.collections.list.{List}\n\nfn main()\n    let l = List([1, 2])\n    \
         for i in l.keys()\n        println(f\"{i}\")\n",
    );

    let envelope = plan(fixture.path());

    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_033");
    assert!(
        diagnostic.repair.is_none(),
        "a sequence's iteration is not its keys, so no edit is determined, got {:?}",
        diagnostic.repair
    );
    assert!(
        diagnostic.help.is_some(),
        "the help that names a `for` loop should still be offered"
    );
}

#[test]
fn test_text_joined_to_a_value_is_repaired_into_a_formatted_string() {
    let fixture = Fixture::new(
        "concat-to-formatted-string",
        "fn main()\n    let qty = 3\n    let prefix = \"count\"\n    let line = prefix + \" \" + \
         qty\n    println(line)\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["concat-to-formatted-string"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture
            .contents()
            .contains("let line = f\"{prefix} {qty}\""),
        "the join should have become one formatted string, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_a_join_that_already_holds_a_formatted_string_is_not_repaired() {
    // A formatted string cannot nest inside a hole, so there is no rewrite of
    // this shape to offer.
    let fixture = Fixture::new(
        "concat-with-a-formatted-string",
        "fn main()\n    let qty = 3\n    let line = f\"n={qty}\" + qty\n    println(line)\n",
    );

    let envelope = plan(fixture.path());

    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_002");
    assert!(
        diagnostic.repair.is_none(),
        "a formatted string cannot be quoted into another one, got {:?}",
        diagnostic.repair
    );
}

#[test]
fn test_a_bare_variant_pattern_is_repaired_into_a_qualified_one() {
    let fixture = Fixture::new(
        "qualify-variant-pattern",
        "fn get() Result<int, String>\n    Result.Ok(1)\n\nfn main()\n    match get()\n        \
         Ok(n): println(f\"{n}\")\n        Result.Err(e): println(e)\n",
    );

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["qualify-variant-pattern"]);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        fixture.contents().contains("Result.Ok(n):"),
        "the pattern should now name its enum, got: {}",
        fixture.contents()
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

#[test]
fn test_every_bare_arm_of_one_match_is_qualified_by_a_single_run() {
    // The arms are one mistake repeated, so they travel on one repair. Applying
    // it once has to leave nothing for a second run to find.
    let fixture = Fixture::new(
        "qualify-every-arm",
        "fn get() Result<int, String>\n    Result.Ok(1)\n\nfn main()\n    match get()\n        \
         Ok(n): println(f\"{n}\")\n        Err(e): println(e)\n",
    );

    let envelope = plan(fixture.path());
    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_038");
    let repair = diagnostic
        .repair
        .as_ref()
        .expect("the bare arms should carry a repair");
    assert_eq!(
        repair.edits.len(),
        2,
        "both arms should be edited by the one repair"
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");
    assert!(
        checks_clean(fixture.path()),
        "one run should be enough, got: {}",
        fixture.contents()
    );
}

#[test]
fn test_an_optional_operand_is_told_to_unwrap_and_offered_no_rewrite() {
    // Text joined to an optional trips both readings — the operand is optional
    // and one side is text. Unwrapping is the remedy the help names, so the
    // report must not also carry an edit that interpolates it instead.
    let fixture = Fixture::new(
        "optional-operand",
        "fn pick(flag bool) int?\n    if flag\n        return 1\n    None\n\nfn main()\n    let v \
         = pick(true)\n    let s = \"x\" + v\n    println(s)\n",
    );

    let envelope = plan(fixture.path());

    let diagnostic = diagnostic_with_code(&envelope, "MER_TYP_002");
    assert!(
        diagnostic
            .help
            .as_deref()
            .is_some_and(|help| help.contains("unwrapped")),
        "the optional reading is the one that answers first, got {:?}",
        diagnostic.help
    );
    assert!(
        diagnostic.repair.is_none(),
        "no edit may contradict the help that was given, got {:?}",
        diagnostic.repair
    );
}

/// Six uses of one unimported name raise six diagnostics, each carrying the
/// same `add-import` repair inserting the same line at the same offset. The
/// file needs that line once: an edit repeated is still one edit, and applying
/// it as many times as it was reported writes a file no one asked for.
#[test]
fn test_one_import_repaired_once_however_many_diagnostics_asked_for_it() {
    let fixture = Fixture::new(
        "repeated-import-repair",
        "fn main()\n    assert_eq(1, 1)\n    assert_eq(2, 2)\n    assert_eq(3, 3)\n    \
         assert_eq(4, 4)\n    assert_eq(5, 5)\n    assert_eq(6, 6)\n",
    );

    let envelope = plan(fixture.path());
    assert!(
        envelope.diagnostics.len() > 1,
        "the fixture must raise more than one diagnostic to be testing anything, got {}",
        envelope.diagnostics.len()
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the repair should succeed");

    let contents = fixture.contents();
    let imports = contents
        .lines()
        .filter(|line| line.starts_with("use "))
        .count();
    assert_eq!(
        imports, 1,
        "the file should gain one import, got:\n{}",
        contents
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean, got:\n{}",
        contents
    );
}

/// A diagnostic whose help names the exact token to write instead hands over
/// the edit as well as the sentence. These prove the two shapes that name one:
/// a near-miss member, and a binding nothing reads.
#[test]
fn test_a_near_miss_member_is_renamed_to_the_one_the_receiver_declares() {
    assert_eq!(
        repaired(
            "rename-member",
            "fn main()\n    let accounts = [1, 2, 3]\n    println(f\"{accounts.len()}\")\n",
            "rename-to-suggestion",
        ),
        "fn main()\n    let accounts = [1, 2, 3]\n    println(f\"{accounts.length()}\")\n"
    );
}

#[test]
fn test_an_unread_local_gains_the_underscore_its_help_names() {
    assert_eq!(
        repaired(
            "underscore-local",
            "fn main()\n    let total = 42\n",
            "underscore-unread-binding",
        ),
        "fn main()\n    let _total = 42\n"
    );
}

/// Renaming a parameter rewrites the signature callers name their arguments
/// through, so the same edit that is a local one on a local is api-changing on
/// a parameter. The repair is offered either way; only the second needs the
/// caller to say they accept the risk.
#[test]
fn test_an_unread_parameter_is_underscored_only_when_the_risk_is_accepted() {
    let source = "fn greet(name String)\n    println(\"hi\")\n\nfn main()\n    greet(\"a\")\n";
    let fixture = Fixture::new("underscore-parameter", source);

    let envelope = plan(fixture.path());
    assert_eq!(repair_ids(&envelope), vec!["underscore-unread-binding"]);
    assert_eq!(
        diagnostic_with_code(&envelope, "MER_TYP_072")
            .fix_safety
            .as_deref(),
        Some("api-changing"),
        "renaming a parameter changes the signature"
    );

    let (_, _, refused) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(
        !refused,
        "the rename should be withheld without --allow-risky"
    );
    assert_eq!(
        fixture.contents(),
        source,
        "nothing should have been written"
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes", "--allow-risky"]);
    assert!(ok, "the rename should be applied once the risk is accepted");
    assert_eq!(
        fixture.contents(),
        "fn greet(_name String)\n    println(\"hi\")\n\nfn main()\n    greet(\"a\")\n"
    );
    assert!(
        checks_clean(fixture.path()),
        "the repaired source should check clean"
    );
}

/// `let-to-var` refuses a statement binding several names because one keyword
/// governs them all. The underscore is the other case: each name is its own
/// text, so the one nothing reads is the only one rewritten.
#[test]
fn test_only_the_unread_name_of_a_multi_binding_statement_is_underscored() {
    assert_eq!(
        repaired(
            "underscore-one-of-several",
            "fn main()\n    let a = 1, b = 2\n    println(f\"{a}\")\n",
            "underscore-unread-binding",
        ),
        "fn main()\n    let a = 1, _b = 2\n    println(f\"{a}\")\n"
    );
}

#[test]
fn test_a_rename_lands_correctly_after_multi_byte_characters() {
    assert_eq!(
        repaired(
            "rename-after-multibyte",
            "fn main()\n    // ünïcödé\n    let n = [1, 2, 3]\n    println(f\"{n.len()}\")\n",
            "rename-to-suggestion",
        ),
        "fn main()\n    // ünïcödé\n    let n = [1, 2, 3]\n    println(f\"{n.length()}\")\n"
    );
}

/// An import nothing reads hands over the deletion its help names: the whole
/// line when nothing on it is read.
#[test]
fn test_an_unused_import_line_is_removed() {
    assert_eq!(
        repaired(
            "unused-import-line",
            "use system.math\n\nfn main()\n    println(\"done\")\n",
            "remove-unused-import",
        ),
        "\nfn main()\n    println(\"done\")\n"
    );
}

/// A selection whose other names are read loses only the unread name, and
/// keeps its braces.
#[test]
fn test_an_unread_name_is_dropped_from_a_selective_import() {
    assert_eq!(
        repaired(
            "unused-selected-name",
            "use system.math.{wrapping_add, wrapping_sub}\n\nfn main()\n    println(f\"{wrapping_add(1, 2)}\")\n",
            "remove-unused-import",
        ),
        "use system.math.{wrapping_add}\n\nfn main()\n    println(f\"{wrapping_add(1, 2)}\")\n"
    );
}

/// Every unread name in a selection is dropped in one run, wherever it sits,
/// alongside a whole unused line: one `fix --apply` leaves a file `check`
/// accepts without a warning.
#[test]
fn test_one_apply_removes_every_unused_import_form() {
    let source = "use system.collections.list\nuse system.math.{wrapping_add, wrapping_sub, wrapping_mul}\n\nfn main()\n    println(f\"{wrapping_sub(5, 2)}\")\n";
    let fixture = Fixture::new("unused-import-forms", source);

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the import repairs should succeed");
    assert_eq!(
        fixture.contents(),
        "use system.math.{wrapping_sub}\n\nfn main()\n    println(f\"{wrapping_sub(5, 2)}\")\n"
    );
    let envelope = plan(fixture.path());
    assert!(
        envelope.diagnostics.is_empty(),
        "the repaired file should report nothing, got {:?}",
        envelope.diagnostics
    );
}

/// A number stored at a narrower width hands over the cast its help names.
#[test]
fn test_a_wider_integer_initializer_is_cast_to_the_declared_width() {
    assert_eq!(
        repaired(
            "cast-initializer",
            "fn main()\n    let n int = 5\n    let y i32 = n\n    println(f\"{y}\")\n",
            "cast-to-slot-type",
        ),
        "fn main()\n    let n int = 5\n    let y i32 = n as i32\n    println(f\"{y}\")\n"
    );
}

/// A compound value is parenthesised, so the cast converts all of it rather
/// than its last operand.
#[test]
fn test_a_compound_value_stored_in_a_narrower_element_is_cast_whole() {
    assert_eq!(
        repaired(
            "cast-element-store",
            "use system.collections.list\n\nfn main()\n    let n int = 5\n    var xs = List<i32>()\n    xs.push(0)\n    xs[0] = n * 2\n    println(f\"{xs[0]}\")\n",
            "cast-to-slot-type",
        ),
        "use system.collections.list\n\nfn main()\n    let n int = 5\n    var xs = List<i32>()\n    xs.push(0)\n    xs[0] = (n * 2) as i32\n    println(f\"{xs[0]}\")\n"
    );
}

#[test]
fn test_a_wider_argument_is_cast_to_the_parameter_width() {
    assert_eq!(
        repaired(
            "cast-argument",
            "fn take(x u32) u32\n    return x\n\nfn main()\n    let n int = 5\n    println(f\"{take(n)}\")\n",
            "cast-to-slot-type",
        ),
        "fn take(x u32) u32\n    return x\n\nfn main()\n    let n int = 5\n    println(f\"{take(n as u32)}\")\n"
    );
}

#[test]
fn test_a_wider_float_is_cast_to_the_declared_width() {
    assert_eq!(
        repaired(
            "cast-float",
            "fn main()\n    let d float = 1.5\n    var g f32 = 0.0\n    g = d\n    println(f\"{g}\")\n",
            "cast-to-slot-type",
        ),
        "fn main()\n    let d float = 1.5\n    var g f32 = 0.0\n    g = d as f32\n    println(f\"{g}\")\n"
    );
}

/// What `miri run` prints for `path`.
fn run_stdout(path: &Path) -> String {
    let output = miri_cmd()
        .arg("run")
        .arg(path)
        .output()
        .expect("failed to run the run command");
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Statements written beside `main` move into it, in order, ahead of what it
/// already does, and print what they were written to print.
#[test]
fn test_statements_beside_main_move_to_its_start() {
    let source = "let n = 5\nprintln(\"start\")\n\nfn main()\n    println(f\"{n}\")\n";
    let fixture = Fixture::new("move-into-main", source);
    let ids = repair_ids(&plan(fixture.path()));
    assert_eq!(
        ids,
        vec!["move-into-main", "move-into-main"],
        "one per stray statement"
    );

    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the move should succeed");
    assert_eq!(
        fixture.contents(),
        "\nfn main()\n    let n = 5\n    println(\"start\")\n    println(f\"{n}\")\n"
    );
    assert_eq!(run_stdout(fixture.path()), "start\n5\n");
}

/// A statement spanning several lines moves with all of them, re-indented,
/// wherever in the file it was written.
#[test]
fn test_a_block_statement_after_main_moves_whole() {
    assert_eq!(
        repaired(
            "move-loop-into-main",
            "fn main()\n    println(\"in main\")\n\nfor i in 0..2\n    println(f\"{i}\")\n",
            "move-into-main",
        ),
        "fn main()\n    for i in 0..2\n        println(f\"{i}\")\n    println(\"in main\")\n\n"
    );
}

/// Moving a binding another function reads would take the name away from it,
/// so no move is offered.
#[test]
fn test_no_move_is_offered_when_another_function_reads_the_name() {
    let fixture = Fixture::new(
        "move-refused-other-reader",
        "let n = 5\n\nfn helper() int\n    return n\n\nfn main()\n    println(f\"{helper()}\")\n",
    );
    assert!(!repair_ids(&plan(fixture.path())).contains(&"move-into-main".to_string()));
}

/// `main` declaring the same name would see it declared twice.
#[test]
fn test_no_move_is_offered_when_main_declares_the_name() {
    let fixture = Fixture::new(
        "move-refused-redeclared",
        "let n = 5\n\nfn main()\n    let n = 6\n    println(f\"{n}\")\n",
    );
    assert!(!repair_ids(&plan(fixture.path())).contains(&"move-into-main".to_string()));
}

/// An `else` and the `while` closing a `do` sit at the statement's own
/// indentation and still belong to it, so they move with it.
#[test]
fn test_continuation_lines_move_with_their_statement() {
    let fixture = Fixture::new(
        "move-continuations",
        "var x = 2\nif x > 1\n    println(\"big\")\nelse\n    println(\"small\")\ndo\n    x -= 1\nwhile x > 0\n\nfn main()\n    println(\"end\")\n",
    );
    let (_, _, ok) = fix(fixture.path(), &["--apply", "--yes"]);
    assert!(ok, "applying the move should succeed");
    assert_eq!(run_stdout(fixture.path()), "big\nend\n");
    assert_eq!(
        fixture.contents(),
        "\nfn main()\n    var x = 2\n    if x > 1\n        println(\"big\")\n    else\n        println(\"small\")\n    do\n        x -= 1\n    while x > 0\n    println(\"end\")\n"
    );
}

/// Writing an element through a `let` binding is the same mistake as
/// reassigning it, so it hands over the same `var`.
#[test]
fn test_an_element_written_through_a_let_binding_rebinds_it_as_var() {
    assert_eq!(
        repaired(
            "let-to-var-element",
            "use system.collections.list\n\nfn main()\n    let xs = List([1, 2, 3])\n    xs[0] = 9\n    println(f\"{xs[0]}\")\n",
            "let-to-var",
        ),
        "use system.collections.list\n\nfn main()\n    var xs = List([1, 2, 3])\n    xs[0] = 9\n    println(f\"{xs[0]}\")\n"
    );
}

#[test]
fn test_a_field_written_through_a_let_binding_rebinds_it_as_var() {
    assert_eq!(
        repaired(
            "let-to-var-field",
            "struct Point\n    x int\n    y int\n\nfn main()\n    let p = Point(x: 1, y: 2)\n    p.x = 5\n    println(f\"{p.x}\")\n",
            "let-to-var",
        ),
        "struct Point\n    x int\n    y int\n\nfn main()\n    var p = Point(x: 1, y: 2)\n    p.x = 5\n    println(f\"{p.x}\")\n"
    );
}

/// A parameter has no `let` to rewrite, so writing through one offers none.
#[test]
fn test_an_element_written_through_a_parameter_offers_no_rebinding() {
    let fixture = Fixture::new(
        "let-to-var-parameter",
        "use system.collections.list\n\nfn set(xs List<int>)\n    xs[0] = 9\n\nfn main()\n    set(List([1]))\n",
    );
    assert!(!repair_ids(&plan(fixture.path())).contains(&"let-to-var".to_string()));
}
