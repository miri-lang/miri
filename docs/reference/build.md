# Build and Command Line

The `miri` command-line tool is the entry point for compilation. It invokes the full pipeline (lexer, parser, type checker, MIR lowering, code generation, and linking) and reports diagnostics to the user. The `miri explain` subcommand provides detailed guidance for diagnostic codes.

## Build Errors

Errors can occur at any stage of the pipeline. Use the diagnostic code and `miri explain` to understand the root cause and how to fix it. The compiler reports one or more errors before exiting with a non-zero status.

## Diagnostic Code Format

All diagnostic codes follow the format `MER_<AREA>_<NUM>`, where AREA is a two-letter category (LEX, PAR, NAM, IMP, TYP, OWN, MIR, CG, RT, TAR, BLD) and NUM is a zero-padded three-digit decimal number. Each code is stable and never re-assigned.

## Reproducible Builds

`miri determinism check <input>` builds the input twice into separate output directories and compares the produced artifacts byte for byte. Identical artifacts exit 0; any difference is reported as `MER_BLD_003` with the artifact's path, the first differing byte offset, and a short hex window from each build, and exits non-zero. An input that does not compile is reported through the compiler's own diagnostics rather than as a determinism failure.

## Profiling

`miri profile <input>` builds the input as `--profile` builds it, runs it while the runtime samples its CPU time, and lists the source lines with the most samples, each with its self share, its inclusive share and its text, then the functions outside any source line, such as the runtime's. Code an optimized build inlined into its caller is charged to the line it was written on, and inclusively to every line it was inlined through, as if each call had stayed a call. A program built with `--profile` samples itself when `MIRI_PROFILE_OUT` names a file to write its record to, at `MIRI_PROFILE_HZ` samples a second (1000 by default); without the variable it installs nothing. Sampling runs on macOS and Linux, on aarch64 and x86-64; elsewhere the command reports `MER_BLD_027`. A run too short to sample reports `MER_BLD_028`, and one that died `MER_BLD_029`.

## Machine-Readable Output

The compiler can emit diagnostics in JSON format (using appropriate output flags) for tool integration. The JSON schema is stable and versioned.

## Per-Code Detail

Use `miri explain MER_BLD_<code>` for detailed guidance on build and command-line diagnostic codes.
