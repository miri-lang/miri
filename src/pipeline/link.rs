// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Linking a compiled object into an executable: finding the linker and the
//! runtime libraries the program links against.

use super::*;

impl Pipeline {
    /// Link an object file to an executable using the system linker.
    ///
    /// When `required_runtimes` is non-empty, the linker is instructed to
    /// search for and link the corresponding static runtime libraries.
    pub(super) fn link_executable(
        &self,
        object_path: &PathBuf,
        output_path: &PathBuf,
        required_runtimes: &BTreeSet<RuntimeKind>,
        runtime_build: RuntimeBuild,
    ) -> Result<(), CompilerError> {
        let linker_path = resolve_linker()?;
        let linker_path_display = linker_path.display().to_string();
        let mut cmd = Command::new(&linker_path);
        cmd.arg(object_path).arg("-o").arg(output_path);

        // Link the math library (required for sin, cos, pow, etc. on Linux)
        cmd.arg("-lm");
        cmd.arg(DEAD_CODE_LINK_ARG);
        cmd.args(NO_DEBUG_MAP_LINK_ARGS);
        cmd.args(runtime_build.executable_link_args());

        // Link required runtime libraries
        for runtime in required_runtimes {
            let lib_dir = match runtime_build {
                RuntimeBuild::FramePointers if runtime.has_frame_pointer_build() => {
                    frame_pointer_library_dir(runtime)?
                }
                RuntimeBuild::FramePointers | RuntimeBuild::Standard => {
                    runtime_library_dir(runtime)?
                }
            };
            cmd.arg(format!("-L{}", lib_dir.display()));
            cmd.arg(format!("-l{}", runtime.library_name()));
            for arg in runtime.extra_link_args() {
                cmd.arg(arg);
            }
            let dependencies = runtime.link_dependencies().iter();
            for dependency in dependencies.filter(|dep| required_runtimes.contains(dep)) {
                cmd.arg(format!("-l{}", dependency.library_name()));
            }
        }

        let status = cmd.status().map_err(|e| {
            CompilerError::Codegen(format!(
                "Failed to run linker '{}': {}",
                linker_path_display, e
            ))
        })?;

        if !status.success() {
            return Err(CompilerError::Codegen(format!(
                "Linker failed with exit code: {:?}",
                status.code()
            )));
        }

        Ok(())
    }
}

/// Resolves a candidate binary string (from `MIRI_CC` or `CC`) safely.
/// Returns an absolute path if provided, or resolves a bare command name via system `PATH`.
/// Relative paths with directory separators are rejected to prevent CWD hijacking.
fn resolve_binary_path(val: &str) -> Result<PathBuf, CompilerError> {
    let path = PathBuf::from(val);
    if path.is_absolute() {
        return Ok(path);
    }

    if path.components().count() == 1 {
        if let Ok(path_env) = std::env::var("PATH") {
            for dir in std::env::split_paths(&path_env) {
                let candidate = dir.join(&path);
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        // If not found in PATH, return the path object as-is so command execution
        // fails gracefully with a standard command lookup error instead of hard erroring here.
        return Ok(path);
    }

    Err(CompilerError::Codegen(format!(
        "Invalid linker path '{}': relative paths containing directory separators are not allowed",
        val
    )))
}

/// Resolve the path to the linker (cc) using absolute paths or environment variables.
/// This prevents unqualified command execution vulnerabilities.
fn resolve_linker() -> Result<PathBuf, CompilerError> {
    if let Ok(cc) = std::env::var("MIRI_CC") {
        return resolve_binary_path(&cc);
    }
    if let Ok(cc) = std::env::var("CC") {
        return resolve_binary_path(&cc);
    }

    // Default to common absolute paths for 'cc'
    let common_paths = [
        "/usr/bin/cc",
        "/usr/local/bin/cc",
        "/usr/bin/gcc",
        "/usr/bin/clang",
    ];
    for path in common_paths {
        let p = PathBuf::from(path);
        if p.exists() {
            return Ok(p);
        }
    }

    Err(CompilerError::Codegen(
        "Linker 'cc' not found in standard locations. Please set the MIRI_CC or CC environment variable to the absolute path of your linker.".to_string(),
    ))
}

/// Resolve the directory containing the compiled static library for a runtime.
///
/// Searches in order:
/// 1. `MIRI_RUNTIME_DIR` environment variable
/// 2. `src/runtime/<name>/target/release` relative to the compiler binary's
///    ancestor directories (walks up from the binary location, with symlinks
///    resolved).
fn runtime_library_dir(runtime: &RuntimeKind) -> Result<PathBuf, CompilerError> {
    // Check environment variable first
    if let Ok(dir) = std::env::var("MIRI_RUNTIME_DIR") {
        let path = PathBuf::from(dir);
        if path.exists() {
            return Ok(path);
        }
    }

    // Walk up from the compiler binary to find the project root. The binary is
    // often reached through a symlink on `PATH`, so the walk starts from the
    // file the link points at; the link's own directory holds no runtime.
    if let Ok(exe) = std::env::current_exe().map(|exe| std::fs::canonicalize(&exe).unwrap_or(exe)) {
        let mut search = exe.as_path();
        while let Some(parent) = search.parent() {
            let release_candidate = parent
                .join("src")
                .join("runtime")
                .join(runtime.name())
                .join("target")
                .join("release");
            if release_candidate.exists() {
                return Ok(release_candidate);
            }
            let debug_candidate = parent
                .join("src")
                .join("runtime")
                .join(runtime.name())
                .join("target")
                .join("debug");
            if debug_candidate.exists() {
                return Ok(debug_candidate);
            }
            search = parent;
        }
    }

    // Fallback: try relative to CWD
    let cwd_candidate = PathBuf::from("src")
        .join("runtime")
        .join(runtime.name())
        .join("target")
        .join("release");
    if cwd_candidate.exists() {
        return Ok(cwd_candidate);
    }

    Err(CompilerError::Codegen(format!(
        "Could not find runtime library '{}'. Set MIRI_RUNTIME_DIR or build the runtime with `cargo build --release`.",
        runtime.library_name()
    )))
}

/// The directory under a runtime's `target` that `make runtimes` builds its
/// frame-pointer copy into.
const FRAME_POINTER_TARGET: &str = "frame-pointers";

/// Resolve the directory holding a runtime's static library built with frame
/// pointers. Where `MIRI_RUNTIME_DIR` names the standard libraries, as an
/// installed toolchain does, the copies live in its `frame-pointers`
/// directory and nowhere else, so a profile build never pairs them with a
/// runtime from another tree. Otherwise it is
/// `src/runtime/<name>/target/frame-pointers/release`, searched for as
/// [`runtime_library_dir`] searches for the standard one: up from the
/// compiler binary, then under the working directory. A missing library, or
/// one older than the standard library beside it, is an error rather than a
/// reason to link the standard one or stale code.
fn frame_pointer_library_dir(runtime: &RuntimeKind) -> Result<PathBuf, CompilerError> {
    match std::env::var_os("MIRI_RUNTIME_DIR").map(PathBuf::from) {
        Some(dir) if dir.exists() => installed_frame_pointer_library_dir(runtime, &dir),
        _ => built_frame_pointer_library_dir(runtime),
    }
}

/// The `frame-pointers` directory of the runtime directory `runtime_dir`,
/// holding `runtime`'s frame-pointer copy no older than the standard library
/// beside it.
fn installed_frame_pointer_library_dir(
    runtime: &RuntimeKind,
    runtime_dir: &Path,
) -> Result<PathBuf, CompilerError> {
    let library = format!("lib{}.a", runtime.library_name());
    let dir = runtime_dir.join(FRAME_POINTER_TARGET);
    if !dir.join(&library).is_file() {
        return Err(frame_pointer_library_error(
            runtime,
            format!(
                "MIRI_RUNTIME_DIR names {}, so the copy belongs in {}; `make runtimes` builds \
                 it into src/runtime/{}/{}/release.",
                runtime_dir.display(),
                dir.display(),
                runtime.name(),
                Path::new("target").join(FRAME_POINTER_TARGET).display()
            ),
        ));
    }
    if is_older(&dir.join(&library), &runtime_dir.join(&library)) {
        return Err(frame_pointer_library_error(
            runtime,
            format!(
                "The copy in {} is older than the standard runtime beside it; run `make runtimes` \
                 and copy it there again.",
                dir.display()
            ),
        ));
    }
    Ok(dir)
}

/// The frame-pointer copy `make runtimes` builds into a source tree, found up
/// from the compiler binary or under the working directory.
fn built_frame_pointer_library_dir(runtime: &RuntimeKind) -> Result<PathBuf, CompilerError> {
    let target = Path::new("src")
        .join("runtime")
        .join(runtime.name())
        .join("target");
    let library = format!("lib{}.a", runtime.library_name());
    let frame_pointer_dir = |target: &Path| target.join(FRAME_POINTER_TARGET).join("release");
    let exe = std::env::current_exe()
        .map(|exe| std::fs::canonicalize(&exe).unwrap_or(exe))
        .ok();
    let roots = exe.iter().flat_map(|exe| exe.ancestors().skip(1));
    let found = roots
        .chain(std::iter::once(Path::new("")))
        .map(|root| root.join(&target))
        .find(|candidate| frame_pointer_dir(candidate).join(&library).is_file())
        .ok_or_else(|| {
            frame_pointer_library_error(
                runtime,
                format!(
                    "Run `make runtimes` to build it into {}.",
                    frame_pointer_dir(&target).display()
                ),
            )
        })?;
    let dir = frame_pointer_dir(&found);
    if is_older(&dir.join(&library), &found.join("release").join(&library)) {
        return Err(frame_pointer_library_error(
            runtime,
            format!(
                "The copy in {} is older than the standard runtime beside it; run `make runtimes` \
                 to rebuild it.",
                dir.display()
            ),
        ));
    }
    Ok(dir)
}

/// The error a profile build reports when the frame-pointer copy of `runtime`
/// cannot be linked, followed by `remedy`.
fn frame_pointer_library_error(runtime: &RuntimeKind, remedy: String) -> CompilerError {
    CompilerError::Codegen(format!(
        "Could not link runtime library '{}' built with frame pointers, which a --profile \
         build links. {}",
        runtime.library_name(),
        remedy
    ))
}

/// Whether `file` was modified before `other`. A file whose time cannot be
/// read is not called older.
fn is_older(file: &Path, other: &Path) -> bool {
    let modified = |path: &Path| fs::metadata(path).and_then(|meta| meta.modified()).ok();
    matches!((modified(file), modified(other)), (Some(file), Some(other)) if file < other)
}
