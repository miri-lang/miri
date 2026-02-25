# Lexer

The `lexer` module is responsible for the first phase of the Miri compilation process: **Translating raw source code into a stream of tokens.**

## Overview

The Lexer scans the input string character by character (or using regular expressions for complex tokens) and groups them into meaningful `Token`s. These tokens abstract away whitespace (except for indentation) and comments, providing the Parser with a clean, structured stream of data.

Miri's Lexer is **indentation-sensitive**. It emits `Indent` and `Dedent` tokens to reflect block structures, similar to Python.

## Architecture

-   **Token Types (`Token`)**: Defines the unified enum of all valid language tokens (keywords, identifiers, literals, operators, punctuation, indentation). Some token variants carry data (e.g., `String`, `Integer`, `Float`, `Identifier`).
-   **Token Stream (`TokenStream`)**: The core iterator that processes the source string and yields `(Token, Span)` pairs. It handles state transitions, particularly for tracking indentation levels.
-   **Spans (`Span`)**: Every generated token is associated with a `Span` (start and end byte offsets) indicating exactly where it originated in the source file. This is crucial for precise error reporting.
-   **Regex Integration**: Miri supports regular expression literals. The lexer contains specialized logic to correctly parse regex patterns without confusing `/` with the division operator.
-   **F-Strings**: The lexer natively processes formatted strings (`f"..."`), emitting interpolation tokens that the parser can assemble.

## Design Principles

1.  **Zero-Allocation (where possible)**: The lexer minimizes heap allocations. Tokens like identifiers use references (`&str`) to the underlying source string when lifetime constraints allow, or are stored efficiently.
2.  **Stateful Indentation**: The lexer maintains an internal stack of indentation widths to correctly emit `Dedent` tokens when block levels decrease, even if multiple levels drop at once.
3.  **Error Resilience**: Lexical errors (like invalid characters) are tracked, but the lexer attempts to recover and continue tokenizing so that the compiler can report multiple errors in a single run.
