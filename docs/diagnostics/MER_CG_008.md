## Rule

An internal error occurred while lowering the program or generating its code. When the compiler itself fails, the message names the stage and the help asks for a report: send the program that triggered it.

A mistake the compiler recognizes in your program reaches you under the code of the rule it breaks, not this one, even when it is found while lowering a function body.

## Messages

- `internal compiler error: {detail}`
- `Internal codegen error: {detail}`

## Help

- `Please report this at {url}`

## Before

This error has no source-level reproduction: it reports a failure of the compiler itself, not a mistake in the program.

## After

The program is not at fault, so there is no rewrite that makes it correct. Report it with the smallest program that still fails, the command that failed and the compiler version.

## Reference

[Code Generation](../reference/codegen.md)
