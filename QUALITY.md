# Quality Checklist

## Project baseline and repo hygiene

* Uses a modern Rust edition (2021 or 2024) consistently across the workspace.
* `cargo fmt` produces **zero diffs**; formatting is automated and enforced.
* If using Rust 2024, `rustfmt.toml` explicitly sets `style_edition` to avoid tool mismatch across environments. ([doc.rust-lang.org][1])
* `cargo clippy` runs clean in CI (on stable; optionally beta/nightly too).
* `README.md` answers: what it is, who it’s for, quickstart, examples, MSRV, supported platforms, licensing.
* `LICENSE` is clear (Apache-2.0/MIT dual is common in Rust).

---

## Rust style and formatting

* Code follows rustfmt defaults; no hand-formatted “style fights.”
* No “clever” formatting that hides control flow (especially in `match`, `if let`, combinator chains).
* Imports are organized and minimal; avoid unused `use` and wildcard imports in library code.
* Prefer explicitness when it improves comprehension (especially around lifetimes, trait bounds, and generics).

---

## Naming and consistency

* Naming follows Rust conventions: `UpperCamelCase` (types/traits), `snake_case` (values/functions), `SCREAMING_SNAKE_CASE` (consts), etc. ([rust-lang.github.io][2])
* Modules, types, and functions use **domain language** (not implementation language).
* **Exception**: In some cases like `ast_factory` and `parser`, functions may be named as nouns (e.g., `boolean`, `program`) matching the AST node they create or parse, rather than verbs (e.g., `create_boolean`, `parse_program`). This is intentional to improve readability and reduce verbosity in deeply nested structures.
* No “abbrev soup” (`cfg`, `ctx`, `mgr`, `util`, `impl2`, etc.) unless truly standard for the domain.
* Symmetry: if there’s `encode`, there’s `decode`; if there’s `new`, there’s a clear construction story (builder/default/from).
* Error type names and variants are consistent (`Error`, `ParseError`, `ConfigError`, variants like `InvalidFoo`, `MissingBar`).

---

## Errors, results, and robustness

* No `unwrap()` / `expect()` in library code (except tests/examples), unless *explicitly justified* with a comment.
* Errors are actionable: messages include context and do not lose root cause.
* `Result<T, E>` and `Option<T>` usage is idiomatic; avoid sentinel values.
* Logging is optional and structured (don’t spam; don’t log secrets; avoid `println!` in libraries).
* `unsafe` is either absent or very localized, documented, and tested with clear invariants.
* Prefer borrowing (`&T`) over cloning in APIs; return owned values only when justified.

---

## Readability and maintainability

* Functions are short, single-purpose, and named after *what* they do, not *how*.
* Control flow is easy to scan; avoid deeply nested logic (extract helpers).
* Complex logic has comments that explain **intent and invariants**, not restating the code.
* Repeated patterns are factored (helpers, iterators, small types), but not over-abstracted.
* Traits are used for behavior; structs for data. Avoid “God traits”.
* Constructors validate invariants; invalid states are unrepresentable where practical.

---

## Clippy and lint strategy

* Clippy configuration is deliberate: `#![deny(warnings)]` (or equivalent in CI) is used thoughtfully.
* If enabling `clippy::pedantic` / `nursery`, it’s done intentionally with documented exceptions.
* Avoid enabling entire `clippy::restriction` wholesale; cherry-pick restrictions that match the project goals. ([doc.rust-lang.org][3])

---

## Documentation quality

* Every `pub` item that matters has doc comments with: what it does, params/returns, errors, panics, examples.
* Examples compile and run (doctests).
* Safety docs exist for any `unsafe` API: preconditions, invariants, and why it’s sound.
* “Why” is documented for non-obvious decisions (tradeoffs, perf tricks, limitations).

---

## Testing checklist (must-have)

* Unit tests cover core logic and edge cases (boundaries, empty inputs, weird Unicode, overflow-ish scenarios).
* Integration tests cover real user flows (public API usage, file IO boundaries, CLI end-to-end if applicable).
* Tests are deterministic (no time/network randomness unless explicitly controlled/mocked).
* Negative tests exist: invalid inputs, corrupted data, partial failures, retry logic.
* Property-based tests are used where invariants matter (parsers/serializers, state machines).
* Snapshot tests are used for stable outputs (errors, AST pretty-printing, generated code) with a review workflow. ([Docs.rs][4])
* If test runtime matters, use a faster runner (e.g., `cargo nextest`) and keep doctests as a separate step if needed. ([nexte.st][5])
* Coverage is tracked (at least locally or in CI) and used as a signal, not a goal.

---

## CI/CD expectations (open-source friendly)

* CI runs: `fmt`, `clippy`, `test` (unit + integration), `doc` build, and optionally MSRV checks.
* Matrix includes major OS targets if the crate claims cross-platform support.
* `cargo publish --dry-run` (for libraries) passes.
* Minimal reproducible build instructions exist for contributors.

---

### One-liner prompt you can paste into an LLM

> Review this Rust project/code as if you’re a strict open-source maintainer. Produce a checklist-style report with **(a)** violations, **(b)** suggested fixes with examples, **(c)** naming consistency issues, **(d)** API ergonomics risks, and **(e)** missing tests (unit/integration/property/snapshot). Assume we enforce `rustfmt`, `clippy`, modern Rust edition, and Rust naming conventions.
