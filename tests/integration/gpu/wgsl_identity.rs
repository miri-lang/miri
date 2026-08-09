// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Demo compile-parity gate: the source a user reads on the website must be the
//! source that actually compiles, and it must produce the same WebGPU kernels as
//! the repo program.
//!
//! For every web demo this asserts three properties:
//!
//! 1. **Displayed == repo, verbatim.** The website's displayed copy
//!    (`../miri-lang.org/assets/demos/<name>.mi`) is byte-identical to a
//!    contiguous slice of the repo source (`examples/gpu/web/<name>.mi`): the
//!    range from the first `use` line through the line before the `// Native
//!    smoke` tail. The stripped parts — the license/doc header and the
//!    host-side smoke test — are the only bytes the two differ by.
//!
//! 2. **Same WGSL.** The full repo program and the displayed slice compile to
//!    byte-identical WebGPU kernels (entry points, WGSL text, dispatch grids,
//!    bindings, buffers). The stripped parts emit no WGSL, so the kernels a user
//!    compiles from the shown source match the repo bundle exactly.
//!
//! 3. **The published artifacts are current.** The site serves compiled bundles
//!    committed into the website repo, because a static generator cannot run the
//!    Miri compiler at publish time — which makes a stale committed artifact the
//!    obvious way for the page to start lying again. Each
//!    `assets/demos/bundles/<name>.json` must equal a fresh build of the source
//!    shown beside it, and the vendored `assets/js/miri-gpu.js` must equal this
//!    repo's copy.
//!
//! This mechanizes "no drift": editing either copy so the shown source stops
//! matching the repo, or stops producing the same kernels, or leaving a published
//! artifact behind a change, fails the gate. The check reads the sibling website
//! repo by relative path and skips with a log line when it is absent (mirroring
//! the adapter-less GPU-test skips), so it stays green in checkouts without the
//! website tree.

use std::fs;
use std::path::PathBuf;

/// Every web demo, by base name. Kept in lockstep with `examples/gpu/web/*.mi`
/// and the website's `assets/demos/*.mi`.
const WEB_DEMOS: &[&str] = &[
    "mandelbrot",
    "game_of_life",
    "particles",
    "fluid",
    "raymarch",
    "neural",
    "blackhole",
    "wormhole",
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Path to the sibling website repo, or `None` when it is not checked out next
/// to this one.
fn website_root() -> Option<PathBuf> {
    let dir = repo_root().parent()?.join("miri-lang.org");
    dir.is_dir().then_some(dir)
}

/// Path to the sibling website's displayed demo copies, or `None` when the
/// website repo is not checked out next to this one.
fn website_demos_dir() -> Option<PathBuf> {
    let dir = website_root()?.join("assets").join("demos");
    dir.is_dir().then_some(dir)
}

/// The contiguous slice of a repo `.mi` that the website displays: from the
/// first `use ` line through the line before the `// Native smoke` tail. Panics
/// if either boundary is missing — every web demo is expected to carry both.
fn displayed_region(full: &str) -> String {
    let lines: Vec<&str> = full.split_inclusive('\n').collect();
    let start = lines
        .iter()
        .position(|l| l.starts_with("use "))
        .expect("web demo must have a `use` line");
    let end = lines
        .iter()
        .position(|l| l.trim_start().starts_with("// Native smoke"))
        .expect("web demo must have a `// Native smoke` tail");
    let mut region: String = lines[start..end].concat();
    while region.ends_with('\n') {
        region.pop();
    }
    region.push('\n');
    region
}

/// Compile `source` to a web-gpu bundle and return its manifest JSON, with the
/// line-number-dependent `sourceMap` entries and the out-dir-derived program
/// `name` stripped so two builds of equivalent kernels compare equal.
fn canonical_manifest(source: &str) -> serde_json::Value {
    use miri::codegen::backend::BuildTarget;
    use miri::pipeline::{BuildOptions, Pipeline};
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let out = std::env::temp_dir()
        .join("miri_wgsl_identity")
        .join(format!("b_{}_{}", std::process::id(), seq));
    fs::create_dir_all(&out).expect("create bundle dir");

    let opts = BuildOptions {
        target: BuildTarget::WebGpu,
        out_path: Some(out.clone()),
        release: false,
        opt_level: 0,
        cpu_backend: Default::default(),
        emit_native_host: false,
    };
    Pipeline::new()
        .build(source, &opts)
        .expect("web-gpu build should succeed");

    let dir_name = out.file_name().and_then(|n| n.to_str()).unwrap();
    let manifest_path = out.join(format!("{}.json", dir_name));
    let text = fs::read_to_string(&manifest_path).expect("read manifest");
    let mut manifest: serde_json::Value = serde_json::from_str(&text).expect("parse manifest");
    // Drop the program `name` (derived from the temp out-dir) at the top level
    // only — nested `name` keys (buffer names) are meaningful and must match.
    if let Some(obj) = manifest.as_object_mut() {
        obj.remove("name");
    }
    // `sourceMap` entries are Miri line numbers, which shift when the header is
    // stripped from the displayed slice; they carry no kernel semantics.
    strip_source_maps(&mut manifest);
    manifest
}

/// Recursively remove every `sourceMap` key from the manifest tree.
fn strip_source_maps(value: &mut serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            map.remove("sourceMap");
            for v in map.values_mut() {
                strip_source_maps(v);
            }
        }
        serde_json::Value::Array(items) => {
            for v in items {
                strip_source_maps(v);
            }
        }
        _ => {}
    }
}

#[test]
fn website_displayed_source_is_a_verbatim_repo_slice() {
    let Some(site_dir) = website_demos_dir() else {
        eprintln!(
            "skipping wgsl-identity gate: sibling website repo (../miri-lang.org) not present"
        );
        return;
    };

    for name in WEB_DEMOS {
        let repo_path = repo_root().join(format!("examples/gpu/web/{name}.mi"));
        let full =
            fs::read_to_string(&repo_path).unwrap_or_else(|e| panic!("read repo demo {name}: {e}"));
        let region = displayed_region(&full);

        let site_path = site_dir.join(format!("{name}.mi"));
        let displayed = fs::read_to_string(&site_path).unwrap_or_else(|e| {
            panic!(
                "website is missing the displayed copy for `{name}` ({}): {e}. \
                 Regenerate it as the repo's displayed region.",
                site_path.display()
            )
        });

        assert_eq!(
            displayed, region,
            "website's displayed `{name}.mi` is not a verbatim slice of the repo \
             source. The shown program must equal `examples/gpu/web/{name}.mi` \
             from its first `use` line through the line before `// Native smoke`."
        );
    }
}

#[test]
fn website_displayed_source_compiles_to_identical_wgsl() {
    let Some(site_dir) = website_demos_dir() else {
        eprintln!(
            "skipping wgsl-identity gate: sibling website repo (../miri-lang.org) not present"
        );
        return;
    };

    for name in WEB_DEMOS {
        let repo_path = repo_root().join(format!("examples/gpu/web/{name}.mi"));
        let full =
            fs::read_to_string(&repo_path).unwrap_or_else(|e| panic!("read repo demo {name}: {e}"));

        let site_path = site_dir.join(format!("{name}.mi"));
        let displayed = fs::read_to_string(&site_path)
            .unwrap_or_else(|e| panic!("read website demo {name}: {e}"));

        let repo_bundle = canonical_manifest(&full);
        let displayed_bundle = canonical_manifest(&displayed);

        assert_eq!(
            repo_bundle, displayed_bundle,
            "`{name}`: the website's displayed source compiles to different WebGPU \
             kernels than the repo program. Users would not get what they see."
        );
    }
}

/// The manifests the website serves are committed artifacts, so nothing rebuilds
/// them when a demo changes. This is the check that catches one going stale: each
/// published bundle must equal a fresh build of the source shown beside it.
#[test]
fn published_bundles_match_a_fresh_build_of_the_displayed_source() {
    let Some(site_dir) = website_demos_dir() else {
        eprintln!(
            "skipping wgsl-identity gate: sibling website repo (../miri-lang.org) not present"
        );
        return;
    };
    let bundles = site_dir.join("bundles");

    for name in WEB_DEMOS {
        let published_path = bundles.join(format!("{name}.json"));
        let published_text = fs::read_to_string(&published_path).unwrap_or_else(|e| {
            panic!(
                "website is missing the published bundle for `{name}` ({}): {e}. \
                 Regenerate the artifacts with `tools/gen_demo_bundles.py`.",
                published_path.display()
            )
        });
        let mut published: serde_json::Value = serde_json::from_str(&published_text)
            .unwrap_or_else(|e| panic!("published bundle for `{name}` is not valid JSON: {e}"));
        if let Some(obj) = published.as_object_mut() {
            obj.remove("name");
        }
        strip_source_maps(&mut published);

        let displayed = fs::read_to_string(site_dir.join(format!("{name}.mi")))
            .unwrap_or_else(|e| panic!("read website demo {name}: {e}"));

        assert_eq!(
            published,
            canonical_manifest(&displayed),
            "`{name}`: the published bundle does not match a fresh build of the \
             source shown beside it — the committed artifact is stale, so the page \
             runs kernels the reader cannot reproduce. Regenerate with \
             `tools/gen_demo_bundles.py`."
        );
    }
}

/// The website vendors the runtime driver rather than importing it from here, so
/// it is the same class of stale artifact as the manifests — and an easier one to
/// forget, since editing the driver looks like a change to this repo alone.
#[test]
fn published_runtime_driver_matches_this_repo() {
    let Some(site) = website_root() else {
        eprintln!(
            "skipping wgsl-identity gate: sibling website repo (../miri-lang.org) not present"
        );
        return;
    };

    let ours = fs::read_to_string(repo_root().join("assets/web/miri-gpu.js"))
        .expect("read assets/web/miri-gpu.js");
    let published_path = site.join("assets").join("js").join("miri-gpu.js");
    let published = fs::read_to_string(&published_path).unwrap_or_else(|e| {
        panic!(
            "website is missing the vendored runtime driver ({}): {e}. \
             Regenerate the artifacts with `tools/gen_demo_bundles.py`.",
            published_path.display()
        )
    });

    assert_eq!(
        published, ours,
        "the runtime driver the website serves differs from `assets/web/miri-gpu.js`. \
         Regenerate the artifacts with `tools/gen_demo_bundles.py`."
    );
}
