# CLI (Command Line Interface)

The `cli` module is the **entry point to the Miri compiler**.

## Overview

The CLI provides the interface for evaluating, compiling, and testing Miri programs. It serves as the orchestrator, initializing the compilation pipeline (Lexing -> Parsing -> Type Checking -> MIR Lowering -> Codegen) based on user commands.

## Features

-   **`miri run <file.mi>`**: Compiles and executes a specified Miri program.
-   **`miri build <file.mi>`**: Compiles a Miri program to a native executable binary.
-   **`miri check <file.mi>`**: Runs the frontend (Lexer, Parser, Type Checker) to validate code correctness without generating an executable.
-   **`miri explain <CODE>`**: Renders the registry entry for a diagnostic code — the rule it enforces, a before/after pair, and a reference link. `--list` dumps the whole registry instead, one line per code or, with `--format json`, an entry carrying the code, title, severity, area, retired flag, fix-safety and the repairs reachable from it. Fix-safety is the risk floor an edit for that condition would have to clear and is recorded whether or not one exists; the repair list is what says whether `miri fix` can act on the code at all.
-   **`miri fix <file.mi>`**: Reports the repairs the compiler recorded for a file (`--plan`, the default) or writes them (`--apply`). A repair classified as risky is refused unless `--allow-risky` is given.
-   **`miri view <file.mi | module>`**: Reads part of a program instead of all of it. The argument is a file path, or a module name resolved through the same search an `use` statement uses, so `miri view system.string --outline --public` works without knowing where the module sits on disk. `--type <Name>` lists what can be called on a type — its own members and those it inherits from a base class or a trait — and needs no path when the type comes from the prelude; it reads the type table, so it answers only for a program the frontend accepts. `--stdlib-root` prints the roots a module name is searched in and whether each exists. The rest reads source: `--fn <name>` for one function (`Class.method` for a method), `--outline` for every declaration's signature with no bodies, and `--fn <name> --around <text>` for the innermost block holding some text. `--outline --public` drops `runtime` declarations and `private`/`protected` members, leaving the surface a caller can reach. Output is rendered from the parsed AST rather than sliced out of the file, so what a tool reads is canonical and repeatable — which also means it is not what the file says: a type written `List<String>` reads back `[String]`, and comments are gone. `--raw` answers with the file's own bytes instead, each line behind its source line number and a tab, so dropping everything through the first tab of each line gives the file back exactly. Alone it reads the whole file, and it needs no parse — a file that will not parse is when a reader most needs to see what it holds; with `--fn <name>` it reads that one declaration, keeping the indentation and the comments the file has. `--around` reports `MER_BLD_023` when the anchor narrowed nothing, so a read that returned the whole function never passes for a read of one branch. Under `--format json` each span carries two coordinate systems: `start`/`end` index the rendered text, while `line`/`endLine` give the declaration's place in the source file, so a reader can cite what it read and then go back to the file to edit it.
-   **`miri patch <file.mi>`**: Applies edits and re-checks what they produced, in one call. `--replace-in-fn <name> --old <text> --new <text>` replaces one occurrence of `--old` in a function's canonical rendering; `--replace-fn <name> --body-file <path|->` replaces a body wholesale; `--insert-fn <name> --body-file <path|-> [--after <decl>]` adds a declaration the file does not have. The two read their body file differently and each says so in `--help`: `--replace-fn` puts its text under a header the file already has, so the file holds the body alone, while `--insert-fn` writes the `fn` line too. A body file that declares the function being replaced is refused as the mix-up it is rather than spliced in. `--old-file` / `--new-file` carry multi-line text, `-` reading standard input. Repeating the flags batches edits into one apply and one check. `--expect-sha <hex>` refuses to edit a file that has moved on since the caller read it, `--check-only` and `--dry-run` write nothing, and the latter prints the difference the edits would make. An edit is anchored against canonical text but applied to the bytes the author wrote, so comments and spacing outside the replaced range survive; nothing reaches disk unless the edited program checks. A declaration whose canonical form parts from the file — a string written in single quotes, say — cannot be edited that way and is refused with `MER_BLD_010` naming it, but it is still a landmark: `--after` places a declaration beside it, because an insertion beside a declaration only has to know where it ends.
-   **`miri agent`**: Serves JSON-RPC 2.0 over stdin and stdout so one process answers many requests. See [`docs/agent-protocol.md`](../../docs/agent-protocol.md).
-   **`miri skill list`**: Lists all available skills bundled with the compiler. Skills match the compiler version so AI agents never drift from the language the binary accepts. Emits names and descriptions; `--format json` is available.
-   **`miri skill show <name>`**: Writes one skill to standard output — its header carrying this build's version, then the body exactly as the binary carries it — so the output can be redirected into place. A name the build does not carry is reported on standard error, where it cannot be mistaken for content.
-   **`miri skill install [<name>...] [--agent claude|agents|cursor|codex|generic] [--target DIR]`**: Writes skills to the location appropriate for the agent configuration. `--agent claude` (default) writes to `.claude/skills/<name>/SKILL.md`; `agents`, `cursor`, and `codex` write to `.agents/skills/<name>/SKILL.md`; `generic` writes to `skills/<name>/SKILL.md`. If no names are given, all skills are installed. The compiler version is stamped into the installed copy. Re-running with an identical file succeeds without writing; a locally modified file is refused unless `--force` is given. Emits reports per skill; `--format json` is available.
-   **`miri test`**: Discovers `@test` functions in `.mi` files under a directory and runs each one in an isolated subprocess, with optional filtering by `<path>::<test_name>` and a selectable output format (pretty/json). Discovery, execution and reporting live in `src/test_runner/`; see its README.

## Output Formats

All four commands (`check`, `build`, `run`, `test`) accept a `--format` flag to control output style:

-   **`--format pretty`** (default): Human-readable text output with optional ANSI color codes.
-   **`--format json`**: Machine-readable JSON envelope (`DiagnosticsEnvelope`) with all diagnostics and metadata. JSON output is always uncolored regardless of the `--color` flag.

## Color Output

Control ANSI color codes in terminal output with the global `--color` flag:

-   **`--color auto`** (default): Detect TTY and emit colors only if stderr is a terminal.
-   **`--color always`**: Force color codes on (useful when piping to tools that support ANSI).
-   **`--color never`**: Disable all color codes.

Note: JSON format (`--format json`) never emits ANSI escape codes regardless of the color setting.

## One command, three concerns

Each command module separates the work from the writing, because the command
line is no longer the only caller. A command exposes:

-   a **core** that runs the work and returns what it found — no printing, no
    process exit, no panic, so a long-lived session can call it;
-   a **rendering** step that turns that into text or an envelope;
-   a thin **`run`** that joins the two and is what `main.rs` calls.

`miri agent` calls the same cores, which is what keeps a session and the command
line reporting the same thing. `main.rs` marshals arguments and maps an outcome
onto an exit code; it holds no compilation logic.

## Architecture

-   **Argument Parsing**: Utilizes the `clap` crate to define and parse subcommands, arguments, and flags (e.g., debug modes, optimization levels).
-   **Pipeline Invocation**: Bridges the CLI flags to the internal `CompilationPipeline`, steering backend selection and runtime loading.

## Design Principles

1.  **Developer Experience**: Commands should be intuitive and similar to other programming languages' CLIs.
2.  **Quick Feedback**: The `check` command provides immediate syntax and type validation.
3.  **Composability**: Internal components are decoupled so the CLI handles only presentation and flag parsing, not compilation logic.
