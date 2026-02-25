# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.1.0-alpha.1] - 2026-02-25

### Added
- **Core Language Syntax:** Clean, indentation-sensitive syntax for variables, functions, and control flow.
- **Primitive Types:** Support for `int`, `float`, `bool`, and `String` variables.
- **Variable Declarations:** Implemented immutable (`let`) and mutable (`var`) variable bindings.
- **Control Flow Structures:** `if/else`, `unless`, `while`, `until`, `do-while`, `forever`, and `for..in` loops.
- **Pattern Matching:** Implemented the `match` block syntax for powerful branching based on discriminant or boolean conditions.
- **Functions:** Custom function definitions with typed parameters and return values.
- **Frontend Pipeline:** End-to-end `Lexer` -> `Parser` -> `Type Checker` supporting reliable syntax/type error reporting with accurate spans.
- **MIR Lowering:** Flattening AST and explicit control flow representation.
- **MIR Optimizations:** Basic passes implemented (`SimplifyCfg`, `ConstantPropagation`, `CopyPropagation`, `DeadCodeElimination`, `Perceus RC`).
- **Codegen Engine:** Native binary compilation powered by Cranelift backend.
- **Complete Test Suite:** Over 2,000 successful unit and integration tests enforcing strict compilation capabilities.
- **Documentation:** Initial language specification (`SPEC.md`) and extensive internal architectural READMEs across all submodules.
