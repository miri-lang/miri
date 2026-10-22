// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::path::{Path, PathBuf};

/// A function of the program's own whose loop, on line 8, calls a function of
/// the standard library's.
pub const HASH_LOOP: &str = r#"use system.io
use system.math

fn mix_all(n int) u32
    var acc = 0 as u32
    var i = 0
    while i < n
        acc = acc ^ hash_u32(i as u32)
        i += 1
    acc

fn main()
    println(f"{mix_all(1000)}")
"#;

/// The line of `HASH_LOOP` its loop body is written on.
const HOT_LINE: usize = 8;

/// One range of a function's code, with the file and line it came from.
struct MappedRange {
    start: u32,
    end: u32,
    file: usize,
    line: usize,
}

/// One function of a line map: its link symbol, its code size and its ranges.
struct MappedFunction {
    symbol: String,
    size: u32,
    ranges: Vec<MappedRange>,
}

/// A parsed line map: its file table, by index, and its functions in order.
struct LineMap {
    files: Vec<String>,
    functions: Vec<MappedFunction>,
}

impl LineMap {
    fn parse(text: &str) -> Self {
        let mut lines = text.lines();
        assert_eq!(lines.next(), Some("miri-lines 1"), "no header:\n{text}");
        let mut map = Self {
            files: Vec::new(),
            functions: Vec::new(),
        };
        for line in lines {
            map.read(line, text);
        }
        map
    }

    fn read(&mut self, line: &str, text: &str) {
        if let Some(entry) = line.strip_prefix("file ") {
            let (index, name) = entry.split_once(' ').unwrap();
            assert_eq!(index.parse::<usize>().unwrap(), self.files.len());
            self.files.push(name.to_string());
            return;
        }
        let words: Vec<&str> = line.split_whitespace().collect();
        match words.as_slice() {
            ["fn", index, symbol, size] => {
                assert_eq!(index.parse::<usize>().unwrap(), self.functions.len());
                self.functions.push(MappedFunction {
                    symbol: symbol.to_string(),
                    size: size.strip_prefix("size=").unwrap().parse().unwrap(),
                    ranges: Vec::new(),
                });
            }
            [start, end, location] if line.starts_with("  ") => {
                let (file, line) = location.split_once(':').unwrap();
                self.functions.last_mut().unwrap().ranges.push(MappedRange {
                    start: start.parse().unwrap(),
                    end: end.parse().unwrap(),
                    file: file.parse().unwrap(),
                    line: line.parse().unwrap(),
                });
            }
            _ => panic!("unexpected line {line:?} in:\n{text}"),
        }
    }

    fn file_index(&self, name: &str) -> usize {
        self.files
            .iter()
            .position(|file| file == name)
            .unwrap_or_else(|| panic!("no file {name} in {:?}", self.files))
    }

    fn function(&self, symbol_part: &str) -> &MappedFunction {
        self.function_naming(&[symbol_part])
    }

    /// The first function whose symbol contains every one of `parts`.
    fn function_naming(&self, parts: &[&str]) -> &MappedFunction {
        self.functions
            .iter()
            .find(|function| parts.iter().all(|part| function.symbol.contains(part)))
            .unwrap_or_else(|| {
                let symbols: Vec<&str> = self.functions.iter().map(|f| f.symbol.as_str()).collect();
                panic!("no function naming {parts:?} in {symbols:?}")
            })
    }

    /// Whether every range of `function` maps into the file `name`.
    fn maps_only_into(&self, function: &MappedFunction, name: &str) -> bool {
        let file = self.file_index(name);
        !function.ranges.is_empty() && function.ranges.iter().all(|range| range.file == file)
    }
}

/// Write `files` into `dir` and build the first of them, run from `dir`
/// with `flags`, to `dir/out/prog`, returning the executable's path.
fn build_files(dir: &Path, files: &[(&str, &str)], flags: &[&str]) -> PathBuf {
    for (name, text) in files {
        let path = dir.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }
    let out = dir.join("out").join("prog");
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    miri_cmd()
        .current_dir(dir)
        .arg("build")
        .arg(files[0].0)
        .arg("--out")
        .arg(&out)
        .args(flags)
        .assert()
        .success();
    out
}

/// Write `HASH_LOOP` as `prog.mi` in `dir` and build it with `flags` to
/// `dir/out/prog`, returning the executable's path.
fn build_program(dir: &Path, flags: &[&str]) -> PathBuf {
    build_files(dir, &[("prog.mi", HASH_LOOP)], flags)
}

fn sibling(path: &Path, extension: &str) -> PathBuf {
    let mut file = path.as_os_str().to_owned();
    file.push(extension);
    PathBuf::from(file)
}

fn read_line_map(executable: &Path) -> (String, LineMap) {
    let text = std::fs::read_to_string(sibling(executable, ".lines")).unwrap();
    let map = LineMap::parse(&text);
    (text, map)
}

/// `--emit lines` names the program and the library module by their
/// relative names, and never by where they sit on this machine.
#[test]
fn test_emit_lines_names_files_relative_to_their_roots() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build_program(dir.path(), &["--release", "--emit", "lines"]);
    let (text, map) = read_line_map(&executable);

    assert_eq!(map.files.first().map(String::as_str), Some("prog.mi"));
    map.file_index("system/math.mi");
    let out_dir = dir.path().to_string_lossy();
    for absolute in [
        "/Users",
        "/home",
        "/tmp",
        "/private",
        "/var",
        out_dir.as_ref(),
    ] {
        assert!(
            !text.contains(absolute),
            "line map names {absolute}:\n{text}"
        );
    }
}

/// The loop of the program's own function maps to the line it is written on,
/// inside that function's code, and every function's ranges are sorted and
/// lie within its code.
#[test]
fn test_emit_lines_maps_the_hot_loop_into_its_function() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build_program(dir.path(), &["--release", "--emit", "lines"]);
    let (text, map) = read_line_map(&executable);

    let program = map.file_index("prog.mi");
    let mix_all = map.function("mix_all");
    assert!(
        mix_all
            .ranges
            .iter()
            .any(|range| range.file == program && range.line == HOT_LINE),
        "no range of mix_all maps to prog.mi:{HOT_LINE}:\n{text}"
    );
    for function in &map.functions {
        let mut previous_end = 0;
        for range in &function.ranges {
            assert!(
                previous_end <= range.start && range.start < range.end,
                "unsorted ranges in {}:\n{text}",
                function.symbol
            );
            assert!(range.end <= function.size, "range past {}", function.symbol);
            previous_end = range.end;
        }
    }
}

/// A library function's code maps to the lines of the library file that
/// declares it, read here by their text.
#[test]
fn test_emit_lines_maps_library_code_to_the_library_file() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build_program(dir.path(), &["--release", "--emit", "lines"]);
    let (text, map) = read_line_map(&executable);

    let math = map.file_index("system/math.mi");
    let library = std::fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("src/stdlib/system/math.mi"),
    )
    .unwrap();
    let library_lines: Vec<&str> = library.lines().collect();
    let hash = map.function("hash_u32");
    assert!(!hash.ranges.is_empty(), "hash_u32 has no ranges:\n{text}");
    for range in &hash.ranges {
        assert_eq!(range.file, math, "hash_u32 maps outside its file:\n{text}");
        assert!((1..=library_lines.len()).contains(&range.line));
    }
    assert!(
        hash.ranges
            .iter()
            .any(|range| library_lines[range.line - 1].contains("wrapping_mul(seed")),
        "no range of hash_u32 maps to its first multiply:\n{text}"
    );
}

/// A profile build writes the line map a profiler reads without being asked.
#[test]
fn test_profile_build_writes_its_line_map() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build_program(dir.path(), &["--profile"]);
    let (_, map) = read_line_map(&executable);
    map.function("mix_all");
}

/// Recording where code came from changes none of the code: a release
/// build's object is the same with the line map and without it, and a plain
/// release build writes no line map.
#[test]
fn test_line_map_leaves_release_object_code_unchanged() {
    let plain_dir = tempfile::tempdir().unwrap();
    let mapped_dir = tempfile::tempdir().unwrap();
    let plain = build_program(plain_dir.path(), &["--release"]);
    let mapped = build_program(mapped_dir.path(), &["--release", "--emit", "lines"]);

    assert!(!sibling(&plain, ".lines").exists());
    assert!(sibling(&mapped, ".lines").exists());
    assert!(
        std::fs::read(plain.with_extension("o")).unwrap()
            == std::fs::read(mapped.with_extension("o")).unwrap(),
        "the line map changed the release object code"
    );
}

/// A program that does not compile leaves no line map behind.
#[test]
fn test_emit_lines_writes_nothing_for_a_refused_program() {
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("prog.mi");
    std::fs::write(&source, "fn main()\n    let x int = \"text\"\n").unwrap();
    let out = dir.path().join("prog");
    miri_cmd()
        .arg("build")
        .arg(&source)
        .arg("--out")
        .arg(&out)
        .args(["--emit", "lines"])
        .assert()
        .failure();
    assert!(!sibling(&out, ".lines").exists());
}

/// Comment lines that put the program's own code far enough down its file
/// that an offset into a shorter module, read against the program's text,
/// lands on one of them.
const PADDING: &str = "// This comment keeps the program's code further down its file.\n";

/// A module whose functions hold a default argument, a closure and a
/// function reference, written at offsets the padded program's comments
/// cover.
const STRINGS_MODULE: &str = "fn seed() int
    7

public fn with_default(n int = seed() * 3) int
    n + 1

public fn apply_twice(x int) int
    let add = fn(v int) int: v + x
    add(add(0))

fn triple(v int) int
    v * 3

public fn via_reference(x int) int
    let f = triple
    f(x)
";

/// The program that calls each function of `STRINGS_MODULE`, below
/// `PADDING`.
fn padded_program() -> String {
    let mut program = String::from("use system.io\nuse local.util.strings\n");
    program.push_str(&PADDING.repeat(30));
    program.push_str(
        "fn main()\n    println(f\"{with_default()} {apply_twice(2)} {via_reference(4)}\")\n",
    );
    program
}

fn build_padded_program(dir: &Path) -> (String, LineMap) {
    let program = padded_program();
    let executable = build_files(
        dir,
        &[("prog.mi", &program), ("util/strings.mi", STRINGS_MODULE)],
        &["--release", "--emit", "lines"],
    );
    read_line_map(&executable)
}

/// A default argument written in a module is evaluated in the caller's
/// code, which maps to the caller's own lines: never to a line of the
/// caller's file read off an offset into the module.
#[test]
fn test_default_argument_code_maps_to_the_calling_line() {
    let dir = tempfile::tempdir().unwrap();
    let (text, map) = build_padded_program(dir.path());
    let program = padded_program();
    let main_line = program
        .lines()
        .position(|line| line == "fn main()")
        .unwrap()
        + 1;
    let main = map
        .functions
        .iter()
        .find(|function| function.symbol == "main")
        .unwrap();
    let program_file = map.file_index("prog.mi");
    for range in &main.ranges {
        assert!(
            range.file == program_file && range.line >= main_line,
            "main maps code to {}:{} outside its own lines:\n{text}",
            range.file,
            range.line
        );
    }
}

/// A module named by a `local.` path is listed by its file under the
/// program's directory, marked as the program's own so it cannot be read
/// as a library module of the same path.
#[test]
fn test_local_module_is_named_by_its_file_under_the_program() {
    let dir = tempfile::tempdir().unwrap();
    let (text, map) = build_padded_program(dir.path());
    map.file_index("local:util/strings.mi");
    assert!(!text.contains(&*dir.path().to_string_lossy()), "{text}");
}

/// A closure and a function-reference thunk lowered inside a module's
/// function map to that module's file.
#[test]
fn test_closure_and_function_reference_map_to_their_module() {
    let dir = tempfile::tempdir().unwrap();
    let (text, map) = build_padded_program(dir.path());
    let module = "local:util/strings.mi";
    let closure = map.function_naming(&["$lambda"]);
    assert!(map.maps_only_into(closure, module), "closure:\n{text}");
    let thunk = map.function_naming(&["$fnref"]);
    assert!(map.maps_only_into(thunk, module), "thunk:\n{text}");
}

/// A program given by a path that climbs out of the working directory is
/// still named by its file name alone.
#[test]
fn test_program_outside_the_working_directory_is_named_by_its_file() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("x.mi"), HASH_LOOP).unwrap();
    let work = dir.path().join("work");
    std::fs::create_dir_all(&work).unwrap();
    let out = dir.path().join("x");
    miri_cmd()
        .current_dir(&work)
        .args(["build", "../x.mi", "--out"])
        .arg(&out)
        .args(["--emit", "lines"])
        .assert()
        .success();
    let (text, map) = read_line_map(&out);
    assert_eq!(
        map.files.first().map(String::as_str),
        Some("x.mi"),
        "{text}"
    );
}

/// A program class implementing a library trait, and a library class
/// instantiated at a program type: the trait's default method and the
/// class's method map to the library files that declare them.
const LIBRARY_INSTANCES: &str = r#"use system.io
use system.collections.list
use system.collections.queryable

struct Point
    x int
    y int

class Bag<T> implements Queryable<T>
    private var items List<T>

    fn init()
        self.items = List<T>()

    public fn length() int
        return self.items.length()

    public fn element_at(index int) T
        return self.items.element_at(index)

    public fn add(item T)
        self.items.push(item)

fn main()
    var b = Bag<float>()
    b.add(2.5)
    var points = List<Point>()
    points.push(Point(1, 2))
    let removed = points.remove(Point(1, 2))
    println(f"{b.first() ?? 0.0} {removed}")
"#;

#[test]
fn test_library_defaults_and_instances_map_to_the_library() {
    let dir = tempfile::tempdir().unwrap();
    let executable = build_files(
        dir.path(),
        &[("prog.mi", LIBRARY_INSTANCES)],
        &["--release", "--emit", "lines"],
    );
    let (text, map) = read_line_map(&executable);
    let first = map.function_naming(&["Bag", "first"]);
    assert!(
        map.maps_only_into(first, "system/collections/queryable.mi"),
        "Bag.first:\n{text}"
    );
    let remove = map.function_naming(&["List", "Point", "remove"]);
    assert!(
        map.maps_only_into(remove, "system/collections/list.mi"),
        "List<Point>.remove:\n{text}"
    );
}
