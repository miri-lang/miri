# Contributing to Miri

Thank you for your interest in contributing to Miri! This guide will help you get started and ensure your contributions meet our quality standards.

## Getting Started

1. **Fork the repository** and clone it locally
2. **Set up your environment** — Ensure you have a stable Rust toolchain installed
3. **Build the project** (compiler + all runtime crates):

   ```bash
   make build
   ```

4. **Run tests** to verify everything works:

   ```bash
   make test
   ```

## Before Submitting

Every contribution must pass these checks:

### 1. Format Your Code

```bash
make format
```

Formatting must produce **zero diffs**. This is enforced in CI.

### 2. Run the Linter

```bash
make lint
```

This runs `cargo fmt --check` and `cargo clippy -- -D warnings` across the compiler and all runtime crates. All warnings must be resolved.

### 3. Run Tests

```bash
make test
```

This runs tests across the compiler, standard library, and all runtime crates. All tests must pass. If you're adding new functionality, include appropriate tests.

### 4. GPU Browser Validation (Optional)

If you are contributing GPU kernel code or modifying the WGSL code generator, validate that generated kernels are compatible with browser-class WebGPU via the Tint compiler:

```bash
make gpu-browser-check
```

This is **not** part of `make test` but is enforced in CI before merge. It catches browser-incompatible WGSL (e.g., 64-bit scalar types without proper feature guards, reserved identifiers starting with `__`) that `naga` may accept but Chrome's WebGPU validator rejects.

#### Setting Up Tint Locally

Tint builds from source with CMake + Ninja + a C++ toolchain on all three platforms. The clone/configure/build step is identical; only the dependency install, the binary path, and the environment-variable syntax differ per OS.

**1. Install the build dependencies (CMake, Ninja, Git, a C++ compiler):**

- **Linux (Debian/Ubuntu):**
  ```bash
  sudo apt-get install cmake ninja-build build-essential git
  ```
- **macOS** (Homebrew; the C++ compiler comes from the Xcode Command Line Tools):
  ```bash
  xcode-select --install   # if not already installed
  brew install cmake ninja git
  ```
- **Windows** (winget; install the MSVC C++ toolchain via Visual Studio Build Tools, then run the build from a *x64 Native Tools Command Prompt* / *Developer PowerShell* so CMake finds MSVC and Ninja):
  ```powershell
  winget install Kitware.CMake Ninja-build.Ninja Git.Git
  winget install Microsoft.VisualStudio.2022.BuildTools   # select "Desktop development with C++"
  ```

**2. Build Tint from the pinned immutable Dawn revision** (same on every platform):

```bash
git clone --depth 1 https://chromium.googlesource.com/chromium/src/third_party/dawn dawn
cd dawn
git fetch origin e12c4ee
git checkout e12c4ee

cmake -B build -G Ninja \
  -DTINT_BUILD_CMD_TOOLS=ON \
  -DCMAKE_BUILD_TYPE=Release \
  -DDAWN_BUILD_SAMPLES=OFF \
  -DDAWN_BUILD_TESTS=OFF

cmake --build build -t tint
```

The resulting binary is `build/tint` on Linux/macOS and `build\tint.exe` on Windows.

**3. Point the validation harness at your Tint binary and run the gate:**

- **Linux / macOS** (bash/zsh):
  ```bash
  export MIRI_TINT="$(pwd)/build/tint"
  make gpu-browser-check
  ```
- **Windows** (PowerShell):
  ```powershell
  $env:MIRI_TINT = "$(Get-Location)\build\tint.exe"
  make gpu-browser-check
  ```
  Windows contributors need `make` available (e.g. via MSYS2, Chocolatey, or WSL); without it, run the gate directly with `cargo test --features browser-gpu-gate --test mod browser_validation`.

The harness searches in this order:
- `MIRI_TINT` environment variable (if set)
- `tools/tint/tint` (if present in the repo)
- `tint` on your `PATH`

For more details on naga↔Tint divergence and browser validation rules, see `tests/integration/gpu/BROWSER_VALIDATION.md`.

## Code Style

### Rust Conventions

- **Naming**: Follow Rust conventions — `UpperCamelCase` for types/traits, `snake_case` for functions/variables, `SCREAMING_SNAKE_CASE` for constants
- **Imports**: Keep imports organized and minimal; avoid unused imports and wildcards in library code
- **Formatting**: Follow `rustfmt` defaults; no hand-formatted style overrides

### Naming Guidelines

- Use **domain language** (not implementation language) for modules, types, and functions
- **Exception**: In modules like `ast_factory` and `parser`, functions may be named as nouns matching the AST node they create (e.g., `boolean`, `program`) rather than verbs (`create_boolean`, `parse_program`) to improve readability
- Avoid abbreviation soup (`cfg`, `ctx`, `mgr`, `util`, `impl2`) unless truly standard for the domain
- Maintain symmetry: if there's `encode`, there's `decode`; if there's `new`, there's a clear construction pattern

### Error Handling

- **No `unwrap()` / `expect()` in library code** unless explicitly justified with a comment
- Errors must be actionable with context; don't lose the root cause
- Use `Result<T, E>` and `Option<T>` idiomatically; avoid sentinel values
- Prefer borrowing (`&T`) over cloning; return owned values only when justified

### Readability

- Functions should be short, single-purpose, and named after *what* they do
- Keep control flow scannable; extract helpers to avoid deep nesting
- Comments explain **intent and invariants**, not what the code literally does
- Factor repeated patterns into helpers, but don't over-abstract

## Testing Requirements

### Must-Have Tests

- **Unit tests** cover core logic and edge cases (boundaries, empty inputs, Unicode, overflow scenarios)
- **Integration tests** cover real user flows (public API usage, file I/O, CLI end-to-end)
- **Negative tests** exist for invalid inputs, corrupted data, and partial failures
- Tests are **deterministic** — no time/network randomness unless explicitly controlled

### Testing Best Practices

- Property-based tests for invariants (parsers, serializers, state machines)
- Snapshot tests for stable outputs (errors, AST pretty-printing, generated code)
- Coverage is tracked as a signal, not a goal

### Test Organization

Most test modules include a `utils.rs` file with common utilities. There's also a shared `tests/utils.rs` for cross-module helpers.

## Documentation

- Every `pub` item should have doc comments explaining: what it does, parameters/returns, errors, panics, and examples
- Examples must compile and run (doctests)
- Document the "why" for non-obvious decisions (tradeoffs, performance tricks, limitations)
- Safety docs are required for any `unsafe` API

## Unsafe Code

- `unsafe` should be absent or very localized
- Must be documented with clear invariants
- Must be tested thoroughly

## Project Standards

### Rust Edition

Miri uses a modern Rust edition (2024) consistently across the workspace.

### Clippy Configuration

- Clippy configuration is deliberate
- If enabling `clippy::pedantic` or `clippy::nursery`, do so intentionally with documented exceptions
- Avoid enabling `clippy::restriction` wholesale; cherry-pick restrictions matching project goals

## CI/CD

CI runs the following checks:

- **`build` job** (equivalent to `make lint && make build && make release && make test`):
  - `cargo fmt --check` (compiler + runtimes)
  - `cargo clippy -- -D warnings` (compiler + runtimes)
  - `cargo test` (unit + integration, compiler + runtimes)
  - Documentation build

- **`gpu-browser-gate` job**:
  - Builds Tint from a pinned immutable Dawn revision (browser-class validator)
  - Runs `make gpu-browser-check` to validate every GPU kernel in `examples/gpu/` against Tint
  - Blocks merge if any kernel is browser-invalid

## Questions?

If you have questions about contributing, feel free to open an issue for discussion.
