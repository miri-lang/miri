# Agent conformance corpus

A versioned subset of the conformance suite, published as the contract surface for downstream tooling: pin a Miri toolchain version and assert against this corpus.

Fixtures are executed by `tests/conformance/mod.rs` against the shipped binary. Run them with `make conformance-agent`.

Each fixture carries `// summary:` describing it, plus `// expect: <CODE>` (fail, warn) or `// expect-stdout: <text>` (pass). A live diagnostic code must have a fixture here or an entry in the harness exclusion table with a reason; a code covered by neither fails the completeness gate.


## Error fixtures (`fail/`) — 69

Each program must be rejected with the named code. A fixture whose diagnostic is only raised while the program runs declares `// command: run`.

| Code | Summary |
|---|---|
| MER_IMP_002 | Triggers MER_IMP_002: Imported Name Conflict. |
| MER_LEX_001 | Triggers MER_LEX_001: Invalid Token. |
| MER_LEX_001_macro_bang | Macro invocation with bang operator (Rust-like, not Miri). |
| MER_LEX_002 | Triggers MER_LEX_002: Unclosed Multiline Comment. |
| MER_LEX_003 | Triggers MER_LEX_003: Indentation Mismatch. |
| MER_LEX_005 | Triggers MER_LEX_005: Invalid Binary Literal. |
| MER_LEX_006 | Triggers MER_LEX_006: Invalid Octal Literal. |
| MER_LEX_007 | Triggers MER_LEX_007: Invalid Hex Literal. |
| MER_LEX_008 | Triggers MER_LEX_008: Invalid Number Literal. |
| MER_LEX_011 | Triggers MER_LEX_011: Backslash in Format String. |
| MER_LEX_012 | Triggers MER_LEX_012: Invalid Formatted String Expression. |
| MER_NAM_002 | Triggers MER_NAM_002: Module Not Found. |
| MER_OWN_003 | Triggers MER_OWN_003: Use of Moved Value. |
| MER_OWN_004 | Triggers MER_OWN_004: Unused Value. |
| MER_PAR_001 | Triggers MER_PAR_001: Unexpected Token. |
| MER_PAR_001_colon_annotation | Colon-style type annotation (Rust-like syntax, not Miri). |
| MER_PAR_001_arrow_return_type | Arrow-style return type (Rust-like syntax, not Miri). |
| MER_PAR_001_brace_block | C-style brace block instead of indentation-based. |
| MER_PAR_001_elif | elif keyword (Python-like, not Miri). |
| MER_PAR_001_impl_block | impl block (Rust-like, not Miri). |
| MER_PAR_001_tuple_for_binding | Tuple destructuring in for loop (Python-like, not Miri). |
| MER_PAR_001_destructuring_let | Tuple destructuring in let binding (Python-like, not Miri). |
| MER_PAR_002 | Triggers MER_PAR_002: Unexpected End of File. |
| MER_PAR_003 | Triggers MER_PAR_003: Invalid Type Declaration. |
| MER_PAR_004 | Triggers MER_PAR_004: Invalid Left-Hand Side Expression. |
| MER_PAR_005 | Triggers MER_PAR_005: Invalid Integer Literal. |
| MER_PAR_010 | Triggers MER_PAR_010: Duplicate Match Pattern. |
| MER_PAR_011 | Triggers MER_PAR_011: Missing Match Branches. |
| MER_PAR_012 | Triggers MER_PAR_012: Missing Struct Member Type. |
| MER_PAR_013 | Triggers MER_PAR_013: Missing Struct Members. |
| MER_PAR_014 | Triggers MER_PAR_014: Missing Enum Members. |
| MER_PAR_015 | Triggers MER_PAR_015: Missing Type Expression. |
| MER_PAR_016 | Triggers MER_PAR_016: Missing Constant Initializer. |
| MER_PAR_017 | Triggers MER_PAR_017: Invalid Modifier Combination. |
| MER_PAR_018 | Triggers MER_PAR_018: Unknown Runtime. |
| MER_PAR_019 | Triggers MER_PAR_019: Unsupported Attribute Target. |
| MER_PAR_020 | Triggers MER_PAR_020: Unsupported C-Style Operator. |
| MER_PAR_021 | Triggers MER_PAR_021: Recursion Limit Exceeded. |
| MER_RT_001 | Triggers MER_RT_001: an integer division whose divisor is zero at run time. |
| MER_RT_002 | Triggers MER_RT_002: an integer remainder whose divisor is zero at run time. |
| MER_RT_005 | Triggers MER_RT_005: an assertion failed at run time. |
| MER_TAR_002 | Triggers MER_TAR_002: Unsupported Operation in GPU Code. |
| MER_TAR_005 | Triggers MER_TAR_005: Invalid GPU Parallel Construct. |
| MER_TAR_006 | Triggers MER_TAR_006: GPU Residency Violation. |
| MER_TAR_007 | Triggers MER_TAR_007: Type Not Accelerable. |
| MER_TAR_008 | Triggers MER_TAR_008: GPU-Incompatible Function Signature. |
| MER_TAR_009 | Triggers MER_TAR_009: Value Out of Range for GPU Storage. |
| MER_TYP_002 | Triggers MER_TYP_002: Type Mismatch. |
| MER_TYP_011 | Triggers MER_TYP_011: Non-Exhaustive Enum Match. |
| MER_TYP_012 | Triggers MER_TYP_012: Unknown Attribute. |
| MER_TYP_013 | Triggers MER_TYP_013: Attribute Not Valid. |
| MER_TYP_014 | Triggers MER_TYP_014: Attribute Argument Missing. |
| MER_TYP_015 | Triggers MER_TYP_015: Attribute Argument Extra. |
| MER_TYP_016 | Triggers MER_TYP_016: Invalid Regex Literal. |
| MER_TYP_017 | Triggers MER_TYP_017: Invalid Test Function Signature. |
| MER_TYP_018 | Triggers MER_TYP_018: Missing Required Attribute. |
| MER_TYP_030 | Triggers MER_TYP_030: Argument Count Mismatch. |
| MER_TYP_031 | Triggers MER_TYP_031: Argument Order Error. |
| MER_TYP_032 | Triggers MER_TYP_032: Not Callable. |
| MER_TYP_033 | Triggers MER_TYP_033: Field Not Found. |
| MER_TYP_034 | Triggers MER_TYP_034: Undefined Name. |
| MER_TYP_034_null_literal | null literal (JavaScript/C-like, not Miri). |
| MER_TYP_039 | Triggers MER_TYP_039: Invalid Collection Element Type. |
| MER_TYP_040 | Triggers MER_TYP_040: Invalid Index Operation. |
| MER_TYP_041 | Triggers MER_TYP_041: Invalid Slice Operation. |
| MER_TYP_042 | Triggers MER_TYP_042: Immutability Violation. |
| MER_TYP_043 | Triggers MER_TYP_043: Type Not Found. |
| MER_PAR_001_let_mut | let mut binding (Rust-like, not Miri). |
| MER_TYP_044 | Triggers MER_TYP_044: Type Already Defined. |
| MER_TYP_048 | Triggers MER_TYP_048: Type Inference Failure. |
| MER_TYP_049 | A shared variable is declared outside a GPU function. |
| MER_TYP_050 | A continue statement appears outside any enclosing loop. |
| MER_TYP_051 | Triggers MER_TYP_051: Range Type Mismatch. |
| MER_TYP_053 | Triggers MER_TYP_053: Invalid Vector Builtin. |
| MER_TYP_054 | Triggers MER_TYP_054: Invalid Function Signature. |
| MER_TYP_060 | Triggers MER_TYP_060: Invalid Async or Await Usage. |
| MER_TYP_063 | Triggers MER_TYP_063: Invalid Cast. |
| MER_TYP_065 | Triggers MER_TYP_065: Invalid Constant Arithmetic. |
| MER_TYP_067 | Triggers MER_TYP_067: Type Not Valid in String Interpolation. |
| MER_TYP_068 | Triggers MER_TYP_068: Integer Literal Out of Range. |

## Warning fixtures (`warn/`) — 7

Each program must emit the named code at warning severity and still compile (`ok: true`).

| Code | Summary |
|---|---|
| MER_NAM_001 | Triggers MER_NAM_001: Deprecated Kernel Context Identifier. |
| MER_OWN_001 | Triggers MER_OWN_001: Resource Not Consumed at Scope Exit. |
| MER_TYP_023 | Triggers MER_TYP_023: Unnecessary Double Negation. |
| MER_TYP_024 | Triggers MER_TYP_024: Decrement Operator Not Supported. |
| MER_TYP_025 | Triggers MER_TYP_025: Unnecessary Optional Declaration. |
| MER_TYP_026 | Triggers MER_TYP_026: Deprecated Attribute Spelling. |
| MER_TYP_027 | Triggers MER_TYP_027: @deprecated Attribute. |

## Accepted fixtures (`pass/`) — 82

Near-miss twins of the rejected programs, plus representative end-to-end programs. Each must compile, run, and exit zero.

| Fixture | Summary |
|---|---|
| MER_IMP_002 | Accepted counterpart of MER_IMP_002: Imported Name Conflict does not fire. |
| MER_LEX_002 | Accepted counterpart of MER_LEX_002: Unclosed Multiline Comment does not fire. |
| MER_LEX_003 | Accepted counterpart of MER_LEX_003: Indentation Mismatch does not fire. |
| MER_LEX_005 | Accepted counterpart of MER_LEX_005: Invalid Binary Literal does not fire. |
| MER_LEX_006 | Accepted counterpart of MER_LEX_006: Invalid Octal Literal does not fire. |
| MER_LEX_007 | Accepted counterpart of MER_LEX_007: Invalid Hex Literal does not fire. |
| MER_LEX_008 | Accepted counterpart of MER_LEX_008: Invalid Number Literal does not fire. |
| MER_LEX_010 | Accepted counterpart of MER_LEX_010: Invalid Formatted String does not fire. |
| MER_LEX_011 | Accepted counterpart of MER_LEX_011: Backslash in Format String does not fire. |
| MER_LEX_012 | Accepted counterpart of MER_LEX_012: Invalid Formatted String Expression does not fire. |
| MER_MIR_001 | Accepted counterpart of MER_MIR_001: Unsupported Expression does not fire. |
| MER_MIR_002 | Accepted counterpart of MER_MIR_002: Unsupported Statement does not fire. |
| MER_MIR_005 | Accepted counterpart of MER_MIR_005: Break Outside Loop does not fire. |
| MER_MIR_007 | Accepted counterpart of MER_MIR_007: Unsupported Left-Hand Side does not fire. |
| MER_MIR_008 | Accepted counterpart of MER_MIR_008: Unsupported Operator does not fire. |
| MER_MIR_010 | Accepted counterpart of MER_MIR_010: Invalid GPU Launch Arguments does not fire. |
| MER_MIR_012 | Accepted counterpart of MER_MIR_012: Missing Struct Field does not fire. |
| MER_NAM_001 | Accepted counterpart of MER_NAM_001: Deprecated Kernel Context Identifier does not fire. |
| MER_NAM_002 | Accepted counterpart of MER_NAM_002: Module Not Found does not fire. |
| MER_NAM_003 | Accepted counterpart of MER_NAM_003: Invalid Import Path does not fire. |
| MER_OWN_004 | Accepted counterpart of MER_OWN_004: Unused Value does not fire. |
| MER_PAR_001 | Correct type annotation syntax without colon |
| MER_PAR_002 | Accepted counterpart of MER_PAR_002: Unexpected End of File does not fire. |
| MER_PAR_003 | Accepted counterpart of MER_PAR_003: Invalid Type Declaration does not fire. |
| MER_PAR_005 | Accepted counterpart of MER_PAR_005: Invalid Integer Literal does not fire. |
| MER_PAR_006 | Accepted counterpart of MER_PAR_006: Invalid Float Literal does not fire. |
| MER_PAR_007 | Accepted counterpart of MER_PAR_007: Invalid String Literal does not fire. |
| MER_PAR_008 | Accepted counterpart of MER_PAR_008: Invalid Boolean Literal does not fire. |
| MER_PAR_010 | Accepted counterpart of MER_PAR_010: Duplicate Match Pattern does not fire. |
| MER_PAR_011 | Accepted counterpart of MER_PAR_011: Missing Match Branches does not fire. |
| MER_PAR_012 | Accepted counterpart of MER_PAR_012: Missing Struct Member Type does not fire. |
| MER_PAR_013 | Accepted counterpart of MER_PAR_013: Missing Struct Members does not fire. |
| MER_PAR_014 | Accepted counterpart of MER_PAR_014: Missing Enum Members does not fire. |
| MER_PAR_015 | Accepted counterpart of MER_PAR_015: Missing Type Expression does not fire. |
| MER_PAR_016 | Accepted counterpart of MER_PAR_016: Missing Constant Initializer does not fire. |
| MER_PAR_017 | Accepted counterpart of MER_PAR_017: Invalid Modifier Combination does not fire. |
| MER_PAR_020 | Accepted counterpart of MER_PAR_020: Unsupported C-Style Operator does not fire. |
| MER_PAR_021 | Accepted counterpart of MER_PAR_021: Recursion Limit Exceeded does not fire. |
| MER_RT_001 | Accepted counterpart of MER_RT_001: a non-zero divisor divides normally. |
| MER_RT_002 | Accepted counterpart of MER_RT_002: a non-zero divisor yields a remainder. |
| MER_RT_005 | Accepted counterpart of MER_RT_005: an assertion passes at run time. |
| MER_TAR_002 | Accepted counterpart of MER_TAR_002: Unsupported Operation in GPU Code does not fire. |
| MER_TAR_005 | Accepted counterpart of MER_TAR_005: Invalid GPU Parallel Construct does not fire. |
| MER_TYP_002 | Correct type in variable initialization - string assigned to string |
| MER_TYP_011 | Accepted counterpart of MER_TYP_011: Non-Exhaustive Enum Match does not fire. |
| MER_TYP_012 | Accepted counterpart of MER_TYP_012: Unknown Attribute does not fire. |
| MER_TYP_013 | Accepted counterpart of MER_TYP_013: Attribute Not Valid does not fire. |
| MER_TYP_014 | Accepted counterpart of MER_TYP_014: Attribute Argument Missing does not fire. |
| MER_TYP_015 | Accepted counterpart of MER_TYP_015: Attribute Argument Extra does not fire. |
| MER_TYP_017 | Accepted counterpart of MER_TYP_017: Invalid Test Function Signature does not fire. |
| MER_TYP_018 | Accepted counterpart of MER_TYP_018: Missing Required Attribute does not fire. |
| MER_TYP_019 | Accepted counterpart of MER_TYP_019: GPU Launch Block Dimensions Invalid does not fire. |
| MER_TYP_021 | Accepted counterpart of MER_TYP_021: GPU Function Host Buffer Mismatch does not fire. |
| MER_TYP_022 | Accepted counterpart of MER_TYP_022: GPU Launch Block Size Not Literal does not fire. |
| MER_TYP_023 | Accepted counterpart of MER_TYP_023: Unnecessary Double Negation does not fire. |
| MER_TYP_024 | Accepted counterpart of MER_TYP_024: Decrement Operator Not Supported does not fire. |
| MER_TYP_025 | Accepted counterpart of MER_TYP_025: Unnecessary Optional Declaration does not fire. |
| MER_TYP_026 | Accepted counterpart of MER_TYP_026: Deprecated Attribute Spelling does not fire. |
| MER_TYP_027 | Accepted counterpart of MER_TYP_027: @deprecated Attribute does not fire. |
| MER_TYP_030 | Function called with required argument |
| MER_TYP_031 | Accepted counterpart of MER_TYP_031: Argument Order Error does not fire. |
| MER_TYP_032 | Callable function can be invoked |
| MER_TYP_033 | Accepted counterpart of MER_TYP_033: Field Not Found does not fire. |
| MER_TYP_034 | Defined variable can be referenced |
| MER_TYP_035 | Accepted counterpart of MER_TYP_035: Name Not Visible does not fire. |
| MER_TYP_039 | Accepted counterpart of MER_TYP_039: Invalid Collection Element Type does not fire. |
| MER_TYP_040 | Accepted counterpart of MER_TYP_040: Invalid Index Operation does not fire. |
| MER_TYP_042 | Mutable variable can be reassigned |
| MER_TYP_043 | Accepted counterpart of MER_TYP_043: Type Not Found does not fire. |
| MER_TYP_044 | Accepted counterpart of MER_TYP_044: Type Already Defined does not fire. |
| MER_TYP_048 | Accepted counterpart of MER_TYP_048: Type Inference Failure does not fire. |
| MER_TYP_049 | An ordinary local declaration outside a GPU function is accepted. |
| MER_TYP_050 | A continue statement inside a loop skips the current iteration. |
| MER_TYP_051 | Accepted counterpart of MER_TYP_051: Range Type Mismatch does not fire. |
| MER_TYP_054 | Accepted counterpart of MER_TYP_054: Invalid Function Signature does not fire. |
| MER_TYP_064 | Accepted counterpart of MER_TYP_064: Shadowing Not Allowed does not fire. |
| MER_TYP_065 | Accepted counterpart of MER_TYP_065: Invalid Constant Arithmetic does not fire. |
| MER_TYP_067 | Accepted counterpart of MER_TYP_067: Type Not Valid in String Interpolation does not fire. |
| MER_TYP_068 | Small integer literal fits in range |
| e2e_enum | End-to-end enum match expression |
| e2e_generic_identity | End-to-end identity function without generic |
| e2e_hello | End-to-end hello world program |
| e2e_struct | End-to-end struct construction and field access |
