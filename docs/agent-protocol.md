# The `miri agent` protocol

`miri agent` serves JSON-RPC 2.0 over stdin and stdout. One compiler process
answers many requests, so a tool that drives the compiler pays the start-up cost
once for a session rather than once per invocation.

Every method answers with the same envelope its command-line equivalent prints,
described by [`diagnostics-schema.json`](diagnostics-schema.json). The command
line, this session, and the editor surface that will consume it later therefore
report one thing rather than three.

## Framing

Messages are framed the way a language server frames them: headers, a blank
line, then exactly as many bytes of UTF-8 JSON as the length header declares.

```
Content-Length: 68\r\n
\r\n
{"jsonrpc":"2.0","id":1,"method":"check","params":{"path":"main.mi"}}
```

`Content-Length` is required. Any other header is skipped, so a client may send
the `Content-Type` a language server sends. Closing stdin ends the session.

**stdout carries nothing but response frames.** Everything written for a person
goes to stderr, so a client can read the stream without filtering it.

A message body may declare at most **64 MiB**, and a single header line at most
**8 KiB**. `Content-Length` is the one number a client states before anything
has validated it, and it sizes the buffer the body is read into; a frame over
the limit ends the session rather than being read, because the only thing
saying where that body ends is the number just rejected. No real request comes
near either bound.

## Results and errors

A request the compiler acted on answers with `result`. A request it could not
act on answers with `error`. Exactly one is present.

**A program that fails to compile is a result, not an error.** The compiler was
asked a question and answered it; the answer is an envelope carrying `ok: false`
and the diagnostics. This mirrors `miri check`, which exits 1 and still prints a
well-formed envelope. The `error` member is reserved for a request that could
not be acted on at all — one that did not parse, named a method this build does
not serve, or omitted a parameter.

| Code | Meaning |
|---|---|
| `-32700` | The message did not parse as JSON. |
| `-32600` | The message parsed but is not a JSON-RPC request. |
| `-32601` | The method is not served by this build. |
| `-32602` | A parameter is missing, or names a file that cannot be read. |
| `-32603` | The compiler could not act on a request it understood. |
| `-32800` | The request was withdrawn before it started. |

## Methods

### `initialize`

Opens a session and describes the build serving it. Takes no parameters.

```json
{
  "serverInfo": { "name": "miri", "version": "0.6.0-beta.4", "schemaVersion": 1 },
  "capabilities": {
    "methods": ["initialize", "check", "explain", "fixPlan", "fixApply", "view", "patch", "skillsGet"],
    "reservedMethods": ["tokens", "parse", "graph", "targets", "doctor"],
    "cancellation": true
  }
}
```

`serverInfo.schemaVersion` is the version of the envelope every result on this
session carries — the same number `miri check --format json` emits. A client
compares it against the schema it was written for.

A version gains optional fields and new `command` values as commands land, so a
client must ignore members it does not recognise. The number changes only when
a field it already reads changes shape or meaning, which is what makes it worth
comparing at all.

Calling `initialize` is not required before other methods.

`capabilities.reservedMethods` is **advisory and may grow** between releases, as
the commands behind those names land. A client that caches the handshake should
re-read it after a toolchain upgrade. Growth is never breaking: a reserved
method answers "not yet" both before and after it appears in the list, and a
method only leaves the list by starting to work.

### `check`

`{ "path": "main.mi", "verifyMir": false }` → a `check` envelope.

`verifyMir` is optional and defaults to false. It runs the MIR verification pass
after reference-counting insertion, the same as the CLI's `--verify-mir`.

Warnings never make `ok` false. A check that reports only warnings answers
`ok: true` with the warnings in `diagnostics`.

### `explain`

`{ "code": "MER_TYP_030" }` → an `explain` envelope carrying `explanation`.

A code that is not in the registry is answered as a diagnostic carrying
`MER_BLD_001`, not as a protocol error: the command's whole subject is codes, so
an unrecognised one is something it has an opinion about.

### `fixPlan`

`{ "path": "main.mi" }` → a `fix` envelope. Reports the repairs the compiler
recorded and edits nothing.

### `fixApply`

`{ "path": "main.mi", "allowRisky": false }` → a `fix` envelope.

`ok` says whether the apply succeeded, not whether the file now compiles. Those
are different questions: send `check` afterwards to ask the second one.

`allowRisky` defaults to false. There is no terminal to confirm at over a
session, so the caller says outright whether a repair the compiler classes as
`api-changing`, `target-changing`, or `requires-human-review` may be written.
A refused repair withholds every other repair in the same call rather than
applying a safe subset, and joins the diagnostics as an entry carrying
`MER_BLD_002`.

Only the file named in `path` is written. A repair for a diagnostic raised
inside an imported file is reported and skipped, because the caller never named
that file.

### `skillsGet`

`{ "name": "miri-lang" }` → a `skill` envelope carrying `skills`. Without a
`name`, every skill this build carries comes back.

Each entry is `{ "name", "description", "compilerVersion", "body" }`. The body
is the skill's markdown with its header removed, and it is the same text
`miri skill show` writes, so a tool reading skills over a session and a person
reading them at a terminal cannot be taught different things.

The skills are compiled into the binary, which is the point of serving them
here: `compilerVersion` is the version of the compiler answering, so an agent's
model of the language cannot drift from what this build accepts.

A name the build does not carry is answered as a diagnostic carrying
`MER_BLD_013` with `ok: false`, not as a protocol error — the same shape the
command line reports it in.

### Reserved methods

`tokens`, `parse`, `graph`, `targets`, and
`doctor` are known by name and not yet served. They answer `-32601` with
`data.reserved: true`, so a client can tell a method that is coming from one it
misspelled and say so to its user. An unknown method answers `-32601` with
`data.reserved: false`.

### `$/cancelRequest`

`{ "id": 3 }` — a notification, so it is not answered.

**Cancellation reaches a request that has not started.** A reader takes messages
off stdin while the compiler works, so a cancellation sent during a long compile
is seen immediately and withdraws any queued request naming that identifier;
that request answers `-32800`. **A request already being compiled runs to
completion and answers normally** — the pipeline has no cancellation point to
unwind from, and giving it one would thread a token through every compiler pass
for a transport's benefit.

An identifier is forgotten once it has withdrawn a request, so a client may
reuse it afterwards.

## Example session

```
→ {"jsonrpc":"2.0","id":1,"method":"check","params":{"path":"main.mi"}}
← {"jsonrpc":"2.0","id":1,"result":{"schemaVersion":1,"ok":false,"command":"check","diagnostics":[…]}}

→ {"jsonrpc":"2.0","id":2,"method":"fixPlan","params":{"path":"main.mi"}}
← {"jsonrpc":"2.0","id":2,"result":{"schemaVersion":1,"ok":false,"command":"fix","diagnostics":[…]}}

→ {"jsonrpc":"2.0","id":3,"method":"fixApply","params":{"path":"main.mi"}}
← {"jsonrpc":"2.0","id":3,"result":{"schemaVersion":1,"ok":true,"command":"fix","diagnostics":[…]}}

→ {"jsonrpc":"2.0","id":4,"method":"check","params":{"path":"main.mi"}}
← {"jsonrpc":"2.0","id":4,"result":{"schemaVersion":1,"ok":true,"command":"check","diagnostics":[]}}
```
