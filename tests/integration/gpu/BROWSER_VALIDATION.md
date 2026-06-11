# Browser-class WGSL validation: naga vs. Tint divergence list

Miri value-verifies GPU kernels natively on Metal through **wgpu + naga**. The
shipping target, however, is the **browser** (WebGPU), whose validator is
**Tint** (Chrome/Dawn), not naga. The two validators do not accept the same
language: naga is permissive where Tint enforces strict WebGPU-spec compliance.
WGSL that is naga-valid and Metal-verified can still be **browser-invalid** —
this already happened once (the i64 scalar mapping, superseded 2026-06-07), and
was caught by manual reasoning rather than a gate.

This document is the canonical record of the divergences. The gate that enforces
them lives in `tests/integration/gpu/browser_validation.rs` (runs Tint over every
emitted demo kernel in CI); see that file's module doc-comment for the harness
mechanics.

> **Where the WGSL lives.** The web-gpu bundle embeds each kernel's WGSL inside
> the manifest JSON (`<bundle>.json`, under `seed[].wgsl` and the optional
> `frame.wgsl`). It does **not** write `kernels/*.wgsl` files; any reference to
> that path is stale.

## The divergences

### 1. No 64-bit scalar types in core WGSL (`i64` / `u64` / `f64`)

naga accepts 64-bit scalars; **Tint rejects them** — core WGSL has no `i64`,
`u64`, or `f64`. The browser emission path therefore maps Miri `int` → `i32` and
`float` → `f32`, and dispatch portability is carried by the runtime `elem_size`,
not by a wider WGSL scalar.

- **Do not** emit `i64`/`u64`/`f64` on the browser path. The native
  `shader_int64` / `shader_f64` paths are gated host-side behind device
  `Features` and must never reach a browser bundle.
- Buffer values that exceed the i32 range on a narrow upload are a separate
  correctness concern, guarded independently (out-of-range `int` GPU buffer
  values are rejected, not silently truncated).
- The fake-tint plumbing stub (`tests/fixtures/fake_tint.sh`) encodes exactly
  this rule: it rejects any WGSL containing `i64`, `u64`, or `f64`.

### 2. Reserved `__` identifier prefix

WebGPU reserves all identifiers beginning with a double underscore (`__`); Tint
rejects them, and naga reserves the same prefix for its own lowering. Any
generated or user-chosen WGSL identifier — kernel entry points included — must
**not** start with `__`.

- Auto-generated kernel entry names (`miri_gpu_for_<id>`) are already safe.
- User-chosen `gpu fn` names are not yet validated at the source boundary; a
  `gpu fn __k()` fails late at shader-module compilation rather than at
  type-check. Compile-time rejection with a rename fix-it is a tracked
  follow-up (F33).

### 3. `enable` directive limits

WGSL gates optional features behind `enable` directives, but the set of valid
directives is fixed by the WebGPU spec. In particular:

- **There is no `enable shader_int64;` or `enable shader_f64;`** — those are not
  WGSL extensions. Emitting them is invalid WGSL, not a feature request. 64-bit
  support is negotiated host-side via device `Features`, never via an `enable`
  line in the shader.
- The only browser-relevant optional directive is `enable f16;` (paired with the
  `Features::SHADER_F16` adapter feature). `f16` itself is a tracked follow-up
  (F9); until it ships, the browser path emits no `enable` directives at all.

### 4. Pointer / atomic address-space limits

Core WGSL constrains where pointers and atomics may live:

- **Atomics** (`atomic<i32>` / `atomic<u32>`) are permitted only in the
  `storage` and `workgroup` address spaces, and only over 32-bit integer types
  (no 64-bit atomics in core WGSL). Atomics are not yet emitted by Miri; they are
  a tracked follow-up (F11/F18).
- **Pointers** are restricted to function-local use and a fixed set of address
  spaces; WGSL has no general first-class pointer values. Miri's current
  embarrassingly-parallel map kernels do not synthesize pointer parameters, so
  this is a forward constraint to honor as shared-memory / cooperative features
  (F36) land — not a present-day emission.

## Enforcement

- **Always-run plumbing** uses the committed fake-tint stub, proving WGSL
  extraction and verdict propagation deterministically (covers divergence 1).
- **The real gate** is feature-gated (`browser-gpu-gate`): `all_demo_kernels_pass_tint`
  builds Tint from a pinned Dawn SHA and validates every demo kernel, loud-panicking
  if Tint is unresolved (no silent skip). `make gpu-browser-check` and the
  `gpu-browser-gate` CI job drive it.

Adding any new WGSL feature means checking it against the four categories above
and extending the gate before it can ship in a browser bundle.
