// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Drift-check tests for `runtime_fns::rt` vs. stdlib and the runtime library.
//!
//! # Test 1 — stdlib → `runtime_fns::rt`
//!
//! Every `runtime "core" fn miri_rt_*` declaration in a stdlib `.mi` file must
//! have a matching string constant in [`miri::runtime_fns::rt`].  If a new
//! `.mi` method is added without a corresponding constant, this test fails.
//!
//! # Test 2 — `runtime_fns::rt` → compiled runtime library
//!
//! Every symbol listed in [`miri::runtime_fns::rt::ALL`] must be exported from
//! the compiled runtime static library (`libmiri_runtime_core.a`).  This
//! catches constants that were added to `runtime_fns.rs` for a symbol that was
//! never actually implemented in the runtime.
//!
//! This test requires the runtime to be pre-built (`cargo build --release`
//! inside `src/runtime/core/`).  When the library is not present the test is
//! skipped with an explanatory message rather than failing, so a clean
//! checkout that hasn't built the runtime yet doesn't break `cargo test`.

use miri::runtime_fns::rt;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

// ── helpers ──────────────────────────────────────────────────────────────────

/// Walk `dir` recursively and return the paths of all `.mi` files.
fn collect_mi_files(dir: &Path) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                paths.extend(collect_mi_files(&path));
            } else if path.extension().is_some_and(|e| e == "mi") {
                paths.push(path);
            }
        }
    }
    paths
}

/// Extract every `runtime "core" fn <name>` symbol from a `.mi` source string.
///
/// The pattern is:
/// ```text
/// runtime "core" fn miri_rt_some_name(...)
/// ```
/// We only capture the function name — everything up to the first `(`.
fn extract_runtime_fn_names(source: &str) -> Vec<String> {
    let mut names = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("runtime \"core\" fn ") {
            // rest = "miri_rt_xxx(...) ..."
            if let Some(name) = rest.split('(').next() {
                let name = name.trim();
                if !name.is_empty() {
                    names.push(name.to_owned());
                }
            }
        }
    }
    names
}

/// Return the path to the compiled runtime static library, checking (in order):
/// 1. `MIRI_RUNTIME_DIR` environment variable
/// 2. `<manifest_dir>/src/runtime/core/target/release/`
/// 3. `<manifest_dir>/src/runtime/core/target/debug/`
fn runtime_lib_path() -> Option<PathBuf> {
    let lib_name = if cfg!(target_os = "windows") {
        "miri_runtime_core.lib"
    } else {
        "libmiri_runtime_core.a"
    };

    // 1. Honour the same env var the compiler uses.
    if let Ok(dir) = std::env::var("MIRI_RUNTIME_DIR") {
        let p = PathBuf::from(dir).join(lib_name);
        if p.exists() {
            return Some(p);
        }
    }

    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let base = manifest
        .join("src")
        .join("runtime")
        .join("core")
        .join("target");

    for profile in &["release", "debug"] {
        let p = base.join(profile).join(lib_name);
        if p.exists() {
            return Some(p);
        }
    }

    None
}

/// Run `nm` on `lib` and return the set of exported `miri_rt_*` symbol names.
///
/// On macOS the C symbol table prepends `_` to every name; this function
/// strips that prefix so callers always see bare names like `miri_rt_list_len`.
fn nm_exported_symbols(lib: &Path) -> HashSet<String> {
    let output = std::process::Command::new("nm")
        .arg(lib)
        .output()
        .expect("failed to run `nm` — is it installed?");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut symbols = HashSet::new();

    for line in stdout.lines() {
        // nm output format: `<addr> <type> <name>` or `<addr> <name>` etc.
        // We only care about defined (non-undefined) symbols whose name
        // contains "miri_rt_".
        // Skip undefined references (lines containing " U ").
        if line.contains(" U ") {
            continue;
        }
        if let Some(name) = line.split_whitespace().last() {
            // Strip the leading `_` that macOS (Mach-O) prepends.
            let name = name.strip_prefix('_').unwrap_or(name);
            if name.starts_with("miri_rt_") {
                symbols.insert(name.to_owned());
            }
        }
    }

    symbols
}

// ── tests ────────────────────────────────────────────────────────────────────

/// Every `runtime "core" fn` declaration in stdlib must have a constant in
/// `runtime_fns::rt`.
#[test]
fn stdlib_runtime_fns_have_constants() {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let stdlib_dir = manifest.join("src").join("stdlib");

    let all_constants: HashSet<&str> = rt::ALL.iter().copied().collect();

    let mi_files = collect_mi_files(&stdlib_dir);
    assert!(
        !mi_files.is_empty(),
        "No .mi files found under {stdlib_dir:?}; check CARGO_MANIFEST_DIR"
    );

    let mut missing = Vec::new();

    for path in &mi_files {
        let source = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("Failed to read {path:?}: {e}"));

        for name in extract_runtime_fn_names(&source) {
            if !all_constants.contains(name.as_str()) {
                missing.push(format!("{name}  (declared in {})", path.display()));
            }
        }
    }

    assert!(
        missing.is_empty(),
        "The following `runtime \"core\" fn` symbols are declared in stdlib \
         but have no constant in `runtime_fns::rt::ALL`.\n\
         Add the missing constants to `src/runtime_fns.rs`:\n\n  {}\n",
        missing.join("\n  ")
    );
}

/// Every constant in `runtime_fns::rt::ALL` must be exported from the compiled
/// runtime static library.
///
/// This test is skipped when the runtime library has not been built yet.
/// Build it with:
///
/// ```text
/// cd src/runtime/core && cargo build --release
/// ```
#[test]
fn runtime_fns_constants_exported_from_library() {
    let lib = match runtime_lib_path() {
        Some(p) => p,
        None => {
            eprintln!(
                "SKIP runtime_fns_constants_exported_from_library: \
                 runtime library not found. \
                 Build it with `cd src/runtime/core && cargo build --release`."
            );
            return;
        }
    };

    let exported = nm_exported_symbols(&lib);

    let mut missing: Vec<&str> = rt::ALL
        .iter()
        .copied()
        .filter(|&sym| !exported.contains(sym))
        .collect();
    missing.sort_unstable();

    assert!(
        missing.is_empty(),
        "The following constants in `runtime_fns::rt` are NOT exported from \
         the runtime library ({}).\n\
         Either implement them in `src/runtime/core/` or remove the constants:\n\n  {}\n",
        lib.display(),
        missing.join("\n  ")
    );
}

/// A runtime declaration read far enough to classify its element arguments.
struct RuntimeDecl {
    name: String,
    /// Each parameter's declared type, in order.
    params: Vec<String>,
    /// The declared return type, empty when there is none.
    returns: String,
    /// The type parameters of the class or struct declaring it.
    generics: Vec<String>,
}

/// The type parameter names of a `class`/`struct` header line — empty for one
/// that declares none — or `None` when the line is not such a header.
fn declared_generics(line: &str) -> Option<Vec<String>> {
    let header = line
        .trim()
        .trim_start_matches("public ")
        .trim_start_matches("abstract ");
    let header = header
        .strip_prefix("class ")
        .or_else(|| header.strip_prefix("struct "))?;
    let name_end = header.find([' ', '<']).map_or(header.len(), |i| i);
    let rest = &header[name_end..];
    let (Some(open), Some(close)) = (rest.find('<'), rest.find('>')) else {
        return Some(Vec::new());
    };
    if !rest[..open].trim().is_empty() {
        return Some(Vec::new());
    }
    Some(
        rest[open + 1..close]
            .split(',')
            .filter_map(|g| g.split_whitespace().next().map(str::to_owned))
            .collect(),
    )
}

/// Split `text` at the commas that are not inside `<…>` or `(…)`.
fn split_top_level(text: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let (mut depth, mut current) = (0i32, String::new());
    for c in text.chars() {
        match c {
            '<' | '(' => depth += 1,
            '>' | ')' => depth -= 1,
            ',' if depth == 0 => {
                parts.push(std::mem::take(&mut current));
                continue;
            }
            _ => {}
        }
        current.push(c);
    }
    if !current.trim().is_empty() {
        parts.push(current);
    }
    parts
}

/// Every `runtime "core" fn` a stdlib source declares, with its signature.
fn runtime_decls(source: &str) -> Vec<RuntimeDecl> {
    let mut generics = Vec::new();
    let mut decls = Vec::new();
    for line in source.lines() {
        if let Some(found) = declared_generics(line) {
            generics = found;
        }
        let Some(rest) = line.trim().strip_prefix("runtime \"core\" fn ") else {
            continue;
        };
        let (Some(open), Some(close)) = (rest.find('('), rest.rfind(')')) else {
            continue;
        };
        decls.push(RuntimeDecl {
            name: rest[..open].trim().to_owned(),
            params: split_top_level(&rest[open + 1..close])
                .iter()
                .map(|p| {
                    p.trim()
                        .split_once(' ')
                        .map_or("", |(_, ty)| ty)
                        .trim()
                        .to_owned()
                })
                .collect(),
            returns: rest[close + 1..].trim().to_owned(),
            generics: generics.clone(),
        });
    }
    decls
}

/// A runtime entry point that takes or hands back an element must be classified
/// as doing so, or its element crosses as a raw value word and a wide one is
/// silently cut short.
///
/// The stdlib declaration says which arguments are elements: those typed as the
/// declaring class's own type parameter (`element T`), and a return of that
/// parameter is an element handed back. Both directions are checked, so a new
/// entry point cannot be left unclassified and a classification cannot outlive
/// the declaration it describes.
#[test]
fn runtime_element_arguments_are_classified() {
    use miri::runtime_fns::{element_positions, returns_element_value};
    let stdlib_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");
    let mut wrong = Vec::new();
    for path in collect_mi_files(&stdlib_dir) {
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("Failed to read {path:?}: {e}"));
        for decl in runtime_decls(&source) {
            let is_element = |ty: &str| decl.generics.iter().any(|g| g == ty);
            let declared: Vec<usize> = (0..decl.params.len())
                .filter(|&i| is_element(&decl.params[i]))
                .collect();
            let classified = element_positions(&decl.name).to_vec();
            if declared != classified {
                wrong.push(format!(
                    "{}: element arguments at {declared:?}, classified at {classified:?}",
                    decl.name
                ));
            }
            if is_element(&decl.returns) != returns_element_value(&decl.name) {
                wrong.push(format!(
                    "{}: returns `{}`, but returns_element_value says {}",
                    decl.name,
                    decl.returns,
                    returns_element_value(&decl.name)
                ));
            }
        }
    }
    assert!(
        wrong.is_empty(),
        "runtime entry points whose element arguments disagree with \
         `element_positions` / `returns_element_value` in src/runtime_fns.rs:\n  {}",
        wrong.join("\n  ")
    );
}
