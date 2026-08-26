# The `miri dev` Streaming Format

The `miri dev` command watches a source file and re-checks it whenever the file or its sibling modules change. Output is streamed as JSONL (JSON Lines) — one JSON object per line — suitable for consumption by editors, CI systems, and development tools.

## Overview

A session checks once at startup, so a tool that attaches learns the current state without having to touch a file first, and then once per change. Nothing is reported by a poll that found nothing: a quiet session is silent, and a batch on the stream always means something changed.

Each batch consists of:
1. One **tick** line opening the batch
2. Zero or more **diagnostic** lines (errors, warnings, notes)
3. One **idle** line closing the batch

A consumer reading the stream can:
- Use the tick line's timestamp to detect when a new check started
- Collect all diagnostics until an idle line appears
- Use the idle line to learn whether the check passed or failed, and how long it took

## The Three Line Types

### Tick Line

```json
{"event":"tick","schemaVersion":1,"ts":0,"path":"/abs/path/main.mi"}
```

Signals the start of a check iteration. Fields:

- **`event`** (string): Always `"tick"`.
- **`schemaVersion`** (u32): Schema version (currently `1`). Attach here to detect format changes mid-stream.
- **`ts`** (u64): Milliseconds elapsed since the watch session began. The first tick is always `ts: 0`. This is a monotonic clock, not wall-clock time.
- **`path`** (string): Absolute path to the file being checked.

### Diagnostic Line

```json
{"severity":"error","code":"MER_TYP_010","message":"type mismatch","path":"/abs/path/main.mi","line":5,"column":10}
```

A single compiler diagnostic (error, warning, or note). The shape is identical to a diagnostic entry in the `DiagnosticsEnvelope` used by other commands. Fields include:

- **`severity`** (string): `"error"`, `"warning"`, or `"note"`.
- **`code`** (string, optional): Diagnostic code like `MER_TYP_010`.
- **`message`** (string): Human-readable message.
- **`path`** (string, optional): Source file path.
- **`line`** (u32, optional): 1-indexed line number.
- **`column`** (u32, optional): 1-indexed column number.
- **`length`** (u32, optional): Span length in bytes.
- **`expected`** (string, optional): For type mismatches, the expected type.
- **`actual`** (string, optional): For type mismatches, the actual type.
- **`help`** (string, optional): Actionable help text.
- **`fixSafety`** (string, optional): Repair risk level.
- **`repair`** (object, optional): Repair information.
- **`related`** (array): Related diagnostics (notes).

Importantly: a diagnostic line has **no `event` field**. This is how a consumer distinguishes it from framing lines.

### Idle Line

```json
{"event":"idle","ok":false,"durationMs":12}
```

Signals the end of a check iteration. Fields:

- **`event`** (string): Always `"idle"`.
- **`ok`** (bool): `true` if the check succeeded (no errors), `false` otherwise. Warnings do not make this `false`.
- **`durationMs`** (u64): Time spent checking this iteration, in milliseconds.

## Parsing and Discrimination

To parse a line, check for the presence of an `event` field:

- If it has `event: "tick"`, parse as a tick line.
- If it has `event: "idle"`, parse as an idle line.
- If it has no `event` field, parse as a `JsonDiagnostic`.

Deserialization will reject a line with an unknown field (the schema enforces `deny_unknown_fields` on all three types), so a malformed line will fail to parse. An unrecognised `event` name is refused the same way, rather than being accepted as a framing line whose kind is unknown. This is intentional: a consumer can rely on the shape.

## Worked Example

Here's a snippet of output from a watch session:

```
{"event":"tick","schemaVersion":1,"ts":0,"path":"/home/user/project/main.mi"}
{"severity":"error","code":"MER_TYP_010","message":"expected int, got String","path":"/home/user/project/main.mi","line":5,"column":8}
{"event":"idle","ok":false,"durationMs":45}
```

The tick shows that a check started at time 0ms on `main.mi`. One error was found. The idle shows that the check took 45ms and failed (`ok: false`).

After the file is edited, the consumer receives:

```
{"event":"tick","schemaVersion":1,"ts":2547,"path":"/home/user/project/main.mi"}
{"event":"idle","ok":true,"durationMs":42}
```

The tick opens 2547ms into the session, which is when the edit was noticed. No diagnostics were emitted, and the idle shows the check passed (`ok: true`) and took 42ms.

Between those two batches the stream carried nothing at all, however long the file sat unedited.

## Atomicity and Reliability

Each line is written atomically:

1. The line is fully formatted in memory as a string.
2. A single write call (including the trailing newline) sends the entire line to stdout.
3. A flush immediately follows to ensure the consumer receives it.

A consumer reading the stream line by line therefore never sees a partial JSON object. A batch cut short — by an interrupt, or by the session ending — ends after a whole line, so the worst a consumer sees is a batch that never closed.

The guarantee rests on a single-threaded writer, not on `PIPE_BUF`: a long diagnostic line can exceed the platform's atomic pipe write, so the flush is what makes the line land, and the absence of any other writer is what keeps it whole.

## stdout and stderr

When running `miri dev --format json`:

- **stdout** carries ONLY JSONL stream lines. Every human-facing byte goes to stderr.
- **stderr** is reserved for human error messages (file not found, parse errors, etc.).

This split allows a machine consumer to redirect stdout to a parser while preserving human-readable errors on stderr.

When running `miri dev --format pretty`:

- **stderr** carries the rendered diagnostics, with their source context.
- **stdout** carries the closing summary of a check that passed, and nothing else.
- **No JSON** is emitted at all; the stream format is reached only through `--format json`.

This is the same split `miri check` uses. A session should not report the same check two different ways.

## Session Termination

The `miri dev` command watches until interrupted (e.g., by SIGINT/Ctrl+C). There is no `{"event":"stop"}` line signaling the end; a stopped session is detected by EOF on the stream.

The reason is robustness: a stop line promised but not delivered (e.g., on a hard SIGKILL) leaves the consumer waiting forever. EOF is always reliable and requires no promise the process can fail to keep.

## Future Extensions

Per-phase timing (lexer, parser, type-check duration) and an incremental cache with hit-rate metrics are future extensions. They will be gated by incrementing `schemaVersion`, so a consumer can:

1. Parse the version from the first tick line (or attach mid-stream via `tail -F`).
2. Adapt its output or caching logic based on version.

Consumers attached before the schema version field is first seen (i.e., before the first tick after they started reading) will not know the version. Robust consumers should either (a) wait for the first tick, or (b) assume `schemaVersion: 1` before the first tick appears.

## Versioning

The `schemaVersion` field lives on the tick line so a consumer attaching mid-stream via `tail -F` learns the version at the next batch boundary without waiting for a global handshake. It matches the `SCHEMA_VERSION` constant in the compiler's diagnostics module, which is the single source of truth.
