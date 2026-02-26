# Contributing to Miri

Thank you for your interest in contributing to Miri! This guide will help you get started and ensure your contributions meet our quality standards.

## Getting Started

1. **Fork the repository** and clone it locally
2. **Set up your environment** — Ensure you have a stable Rust toolchain installed
3. **Build the project:**

   ```bash
   cargo build
   ```

4. **Run tests** to verify everything works:

   ```bash
   cargo test
   cd src/runtime/core && cargo test
   ```

## Before Submitting

Every contribution must pass these checks:

### 1. Format Your Code

```bash
cargo fmt
```

Formatting must produce **zero diffs**. This is enforced in CI.

### 2. Run the Linter

```bash
cargo clippy -- -D warnings
```

All Clippy warnings must be resolved.

### 3. Run Tests

To test the main compiler and standard library:

```bash
cargo test
```

To test the runtime components, you must explicitly change directories:

```bash
cd src/runtime/core
cargo test
```

All tests across components must pass. If you're adding new functionality, include appropriate tests.

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

- `cargo fmt --check`
- `cargo clippy -- -D warnings`  
- `cargo test` (unit + integration)
- Documentation build

## Questions?

If you have questions about contributing, feel free to open an issue for discussion.
