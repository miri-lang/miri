// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Stable diagnostic code registry.
//!
//! This module defines all diagnostic codes used by the Miri compiler.
//! Each code is assigned a permanent identifier in the format `MER_<AREA>_<NUM>`.
//!
//! ## Code Stability
//!
//! Codes are write-once and forever. A code is never re-used, even if the check
//! it corresponds to is removed. When a check is removed, its code is marked
//! `reserved` to prevent future re-use.
//!
//! ## Areas
//!
//! - LEX: Lexer (lexical analysis)
//! - PAR: Parser (syntax analysis)
//! - NAM: Naming and identifiers
//! - IMP: Imports and module loading
//! - TYP: Type checker (type inference and validation)
//! - OWN: Ownership and resource management
//! - MIR: MIR lowering (intermediate representation)
//! - CG: Code generation
//! - RT: Runtime traps and failures
//! - TAR: Target-specific capabilities and limits

use crate::diagnostics::Severity;
use std::str::FromStr;

// Macro-based code registry. This is the single source of truth for all diagnostic codes.
// Each tuple is: (AREA, NUM, VariantName, "Title", Severity, is_reserved)
macro_rules! diagnostics {
    ($($area:expr, $num:expr, $variant:ident, $title:expr, $severity:expr, $reserved:expr),* $(,)?) => {
        /// A stable diagnostic code assigned to a compiler check or error.
        ///
        /// Derives Clone, Copy, Debug, PartialEq, Eq, and Hash so codes can be stored
        /// in sets, used as map keys, and compared for equality without allocation.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        #[non_exhaustive]
        pub enum DiagnosticCode {
            $($variant),*
        }

        impl DiagnosticCode {
            /// Get the wire string representation (e.g., "MER_TYP_001").
            pub fn as_str(&self) -> &'static str {
                match self {
                    $(Self::$variant => concat!("MER_", $area, "_", $num)),*
                }
            }

            /// Get the title for this diagnostic code.
            pub fn title(&self) -> &'static str {
                match self {
                    $(Self::$variant => $title),*
                }
            }

            /// Get the area code (e.g., "LEX", "PAR").
            pub fn area(&self) -> &'static str {
                match self {
                    $(Self::$variant => $area),*
                }
            }

            /// Get the number within the area (e.g., "001", "042").
            pub fn number(&self) -> &'static str {
                match self {
                    $(Self::$variant => $num),*
                }
            }

            /// Get the severity level for this code.
            pub fn severity(&self) -> Severity {
                match self {
                    $(Self::$variant => $severity),*
                }
            }

            /// Check if this code is reserved (will not be re-assigned).
            pub fn is_reserved(&self) -> bool {
                match self {
                    $(Self::$variant => $reserved),*
                }
            }

            /// Return all diagnostic codes in stable order.
            pub fn all() -> &'static [DiagnosticCode] {
                &[
                    $(Self::$variant),*
                ]
            }
        }

        impl std::fmt::Display for DiagnosticCode {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.as_str())
            }
        }

        impl FromStr for DiagnosticCode {
            type Err = String;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                for code in Self::all() {
                    if code.as_str() == s {
                        return Ok(*code);
                    }
                }
                Err(format!("unknown diagnostic code: {}", s))
            }
        }

        // Compile-time uniqueness check via const evaluation.
        // Compares (area, num) pairs using byte-level comparison to ensure no duplicates.
        const _: () = {
            const fn bytes_equal(a: &[u8], b: &[u8]) -> bool {
                if a.len() != b.len() {
                    return false;
                }
                let mut i = 0;
                while i < a.len() {
                    if a[i] != b[i] {
                        return false;
                    }
                    i += 1;
                }
                true
            }

            const fn check_uniqueness() {
                // Each entry is the wire string (area_num pair encoded as bytes)
                const ENTRIES: &[(&str, &str)] = &[
                    $((stringify!($area), stringify!($num))),*
                ];

                let mut i = 0;
                while i < ENTRIES.len() {
                    let mut j = i + 1;
                    while j < ENTRIES.len() {
                        // Check if area and num both match
                        if bytes_equal(ENTRIES[i].0.as_bytes(), ENTRIES[j].0.as_bytes())
                            && bytes_equal(ENTRIES[i].1.as_bytes(), ENTRIES[j].1.as_bytes())
                        {
                            panic!("Duplicate diagnostic code");
                        }
                        j += 1;
                    }
                    i += 1;
                }
            }

            const _: () = { check_uniqueness(); };
        };
    };
}

// Invoke the macro with all diagnostic codes.
// Format: (AREA, NUM, VariantName, "Title", Severity, is_reserved)
diagnostics!(
    // LEX — Lexer (13 codes: 001-013, two reserved)
    "LEX",
    "001",
    LexInvalidToken,
    "Invalid Token",
    Severity::Error,
    false,
    "LEX",
    "002",
    LexUnclosedMultilineComment,
    "Unclosed Multiline Comment",
    Severity::Error,
    false,
    "LEX",
    "003",
    LexIndentationMismatch,
    "Indentation Mismatch",
    Severity::Error,
    false,
    "LEX",
    "004",
    LexUnclosedStringLiteral,
    "Unclosed String Literal",
    Severity::Error,
    true,
    "LEX",
    "005",
    LexInvalidBinaryLiteral,
    "Invalid Binary Literal",
    Severity::Error,
    false,
    "LEX",
    "006",
    LexInvalidOctalLiteral,
    "Invalid Octal Literal",
    Severity::Error,
    false,
    "LEX",
    "007",
    LexInvalidHexLiteral,
    "Invalid Hex Literal",
    Severity::Error,
    false,
    "LEX",
    "008",
    LexInvalidNumberLiteral,
    "Invalid Number Literal",
    Severity::Error,
    false,
    "LEX",
    "009",
    LexInvalidRegexLiteral,
    "Invalid Regex Literal",
    Severity::Error,
    false,
    "LEX",
    "010",
    LexInvalidFormattedString,
    "Invalid Formatted String",
    Severity::Error,
    false,
    "LEX",
    "011",
    LexBackslashInFormatString,
    "Backslash in Format String",
    Severity::Error,
    false,
    "LEX",
    "012",
    LexInvalidFormattedStringExpression,
    "Invalid Formatted String Expression",
    Severity::Error,
    false,
    // Reserved: no lexer check emits this. An over-large integer literal is
    // caught later, by the type checker comparing it against the width of the
    // inferred type, and reported as `MER_TYP_068`.
    "LEX",
    "013",
    LexIntegerLiteralOverflow,
    "Integer Literal Overflow",
    Severity::Error,
    true,
    // PAR — Parser (23 codes: 001-023, two reserved)
    "PAR",
    "001",
    ParUnexpectedToken,
    "Unexpected Token",
    Severity::Error,
    false,
    "PAR",
    "002",
    ParUnexpectedEndOfFile,
    "Unexpected End of File",
    Severity::Error,
    false,
    "PAR",
    "003",
    ParInvalidTypeDeclaration,
    "Invalid Type Declaration",
    Severity::Error,
    false,
    "PAR",
    "004",
    ParInvalidLeftHandSide,
    "Invalid Left-Hand Side Expression",
    Severity::Error,
    false,
    "PAR",
    "005",
    ParInvalidIntegerLiteral,
    "Invalid Integer Literal",
    Severity::Error,
    false,
    "PAR",
    "006",
    ParInvalidFloatLiteral,
    "Invalid Float Literal",
    Severity::Error,
    false,
    "PAR",
    "007",
    ParInvalidStringLiteral,
    "Invalid String Literal",
    Severity::Error,
    false,
    "PAR",
    "008",
    ParInvalidBooleanLiteral,
    "Invalid Boolean Literal",
    Severity::Error,
    false,
    "PAR",
    "009",
    ParInvalidInheritanceIdentifier,
    "Invalid Inheritance Identifier",
    Severity::Error,
    false,
    "PAR",
    "010",
    ParDuplicateMatchPattern,
    "Duplicate Match Pattern",
    Severity::Error,
    false,
    "PAR",
    "011",
    ParMissingMatchBranches,
    "Missing Match Branches",
    Severity::Error,
    false,
    "PAR",
    "012",
    ParMissingStructMemberType,
    "Missing Struct Member Type",
    Severity::Error,
    false,
    "PAR",
    "013",
    ParMissingStructMembers,
    "Missing Struct Members",
    Severity::Error,
    false,
    "PAR",
    "014",
    ParMissingEnumMembers,
    "Missing Enum Members",
    Severity::Error,
    false,
    "PAR",
    "015",
    ParMissingTypeExpression,
    "Missing Type Expression",
    Severity::Error,
    false,
    "PAR",
    "016",
    ParMissingConstantInitializer,
    "Missing Constant Initializer",
    Severity::Error,
    false,
    "PAR",
    "017",
    ParInvalidModifierCombination,
    "Invalid Modifier Combination",
    Severity::Error,
    false,
    "PAR",
    "018",
    ParUnknownRuntime,
    "Unknown Runtime",
    Severity::Error,
    false,
    "PAR",
    "019",
    ParUnsupportedAttributeTarget,
    "Unsupported Attribute Target",
    Severity::Error,
    false,
    "PAR",
    "020",
    ParUnsupportedCStyleOperator,
    "Unsupported C-Style Operator",
    Severity::Error,
    false,
    "PAR",
    "021",
    ParRecursionLimitExceeded,
    "Recursion Limit Exceeded",
    Severity::Error,
    false,
    "PAR",
    "022",
    ParInvalidAssignmentTarget,
    "Invalid Assignment Target",
    Severity::Error,
    true,
    "PAR",
    "023",
    ParUnexpectedOperator,
    "Unexpected Operator",
    Severity::Error,
    true,
    // NAM — Naming and identifiers (1 code: 001)
    "NAM",
    "001",
    NamDeprecatedKernelContextIdentifier,
    "Deprecated Kernel Context Identifier",
    Severity::Warning,
    false,
    "NAM",
    "002",
    NamModuleNotFound,
    "Module Not Found",
    Severity::Error,
    false,
    "NAM",
    "003",
    NamImportPathError,
    "Invalid Import Path",
    Severity::Error,
    false,
    "IMP",
    "001",
    ImpCircularImport,
    "Circular Import",
    Severity::Error,
    false,
    "IMP",
    "002",
    ImpNameConflict,
    "Imported Name Conflict",
    Severity::Error,
    false,
    "IMP",
    "003",
    ImpNameNotFoundInModule,
    "Name Not Found in Module",
    Severity::Error,
    false,
    // TYP — Type checker (29 codes: 001-029, one reserved)
    "TYP",
    "001",
    TypUndefinedVariable,
    "Undefined Variable",
    Severity::Error,
    false,
    "TYP",
    "002",
    TypTypeMismatch,
    "Type Mismatch",
    Severity::Error,
    false,
    "TYP",
    "003",
    TypUnknownType,
    "Unknown Type",
    Severity::Error,
    false,
    "TYP",
    "004",
    TypMissingField,
    "Missing Field",
    Severity::Error,
    false,
    "TYP",
    "005",
    TypMissingVariant,
    "Missing Variant",
    Severity::Error,
    false,
    "TYP",
    "006",
    TypIncompatibleTypesInOperation,
    "Incompatible Types",
    Severity::Error,
    false,
    "TYP",
    "007",
    TypImmutableAssignment,
    "Immutable Assignment",
    Severity::Error,
    false,
    "TYP",
    "008",
    TypMissingReturnStatement,
    "Missing Return",
    Severity::Error,
    false,
    "TYP",
    "009",
    TypInvalidCall,
    "Invalid Call",
    Severity::Error,
    false,
    "TYP",
    "010",
    TypArityMismatch,
    "Arity Mismatch",
    Severity::Error,
    false,
    "TYP",
    "011",
    TypNonExhaustiveMatchNeedsDefault,
    "Non-Exhaustive Enum Match",
    Severity::Error,
    false,
    "TYP",
    "012",
    TypUnknownAttribute,
    "Unknown Attribute",
    Severity::Error,
    false,
    "TYP",
    "013",
    TypAttributeNotValidOnTarget,
    "Attribute Not Valid",
    Severity::Error,
    false,
    "TYP",
    "014",
    TypAttributeArgumentMissing,
    "Attribute Argument Missing",
    Severity::Error,
    false,
    "TYP",
    "015",
    TypAttributeArgumentExtra,
    "Attribute Argument Extra",
    Severity::Error,
    false,
    "TYP",
    "016",
    TypInvalidRegexLiteral,
    "Invalid Regex Literal",
    Severity::Error,
    false,
    "TYP",
    "017",
    TypInvalidTestFunctionSignature,
    "Invalid Test Function Signature",
    Severity::Error,
    false,
    "TYP",
    "018",
    TypMissingRequiredAttribute,
    "Missing Required Attribute",
    Severity::Error,
    false,
    "TYP",
    "019",
    TypGpuLaunchBlockDimensionsInvalid,
    "GPU Launch Block Dimensions Invalid",
    Severity::Error,
    false,
    "TYP",
    "020",
    TypGpuSliceRangeNotBounded,
    "GPU Slice Range Not Bounded",
    Severity::Error,
    false,
    "TYP",
    "021",
    TypGpuFunctionHostBufferMismatch,
    "GPU Function Host Buffer Mismatch",
    Severity::Error,
    false,
    "TYP",
    "022",
    TypGpuLaunchBlockSizeNotLiteral,
    "GPU Launch Block Size Not Literal",
    Severity::Error,
    false,
    "TYP",
    "023",
    TypUnnecessaryDoubleNegation,
    "Unnecessary Double Negation",
    Severity::Warning,
    false,
    "TYP",
    "024",
    TypDecrementOperatorNotSupported,
    "Decrement Operator Not Supported",
    Severity::Warning,
    false,
    "TYP",
    "025",
    TypUnnecessaryOptionalDeclaration,
    "Unnecessary Optional Declaration",
    Severity::Warning,
    false,
    "TYP",
    "026",
    TypDeprecatedAttributeSpelling,
    "Deprecated Attribute Spelling",
    Severity::Warning,
    false,
    "TYP",
    "027",
    TypDeprecatedAttribute,
    "@deprecated Attribute",
    Severity::Warning,
    false,
    "TYP",
    "028",
    TypCustomError,
    "Custom Error",
    Severity::Error,
    true,
    "TYP",
    "029",
    TypAtomicBufferTypeMismatch,
    "Atomic Buffer Type Mismatch",
    Severity::Error,
    false,
    "TYP",
    "030",
    TypArgumentCountMismatch,
    "Argument Count Mismatch",
    Severity::Error,
    false,
    "TYP",
    "031",
    TypArgumentOrderError,
    "Argument Order Error",
    Severity::Error,
    false,
    "TYP",
    "032",
    TypNotCallable,
    "Not Callable",
    Severity::Error,
    false,
    "TYP",
    "033",
    TypFieldNotFound,
    "Field Not Found",
    Severity::Error,
    false,
    "TYP",
    "034",
    TypUndefinedName,
    "Undefined Name",
    Severity::Error,
    false,
    "TYP",
    "035",
    TypNameNotVisible,
    "Name Not Visible",
    Severity::Error,
    false,
    "TYP",
    "036",
    TypGenericArgumentCount,
    "Generic Argument Count Mismatch",
    Severity::Error,
    false,
    "TYP",
    "037",
    TypGenericTypeStructure,
    "Invalid Generic Type Structure",
    Severity::Error,
    false,
    "TYP",
    "038",
    TypEnumVariant,
    "Invalid Enum Variant",
    Severity::Error,
    false,
    "TYP",
    "039",
    TypCollectionElementType,
    "Invalid Collection Element Type",
    Severity::Error,
    false,
    "TYP",
    "040",
    TypIndexOperation,
    "Invalid Index Operation",
    Severity::Error,
    false,
    "TYP",
    "041",
    TypSliceOperation,
    "Invalid Slice Operation",
    Severity::Error,
    false,
    "TYP",
    "042",
    TypImmutabilityViolation,
    "Immutability Violation",
    Severity::Error,
    false,
    "TYP",
    "043",
    TypTypeNotFound,
    "Type Not Found",
    Severity::Error,
    false,
    "TYP",
    "044",
    TypTypeAlreadyDefined,
    "Type Already Defined",
    Severity::Error,
    false,
    "TYP",
    "045",
    TypVariableAlreadyDefined,
    "Variable Already Defined",
    Severity::Error,
    false,
    "TYP",
    "046",
    TypKeywordContextError,
    "Keyword Used in Invalid Context",
    Severity::Error,
    false,
    "TYP",
    "047",
    TypClassInheritance,
    "Invalid Class Inheritance",
    Severity::Error,
    false,
    "TYP",
    "048",
    TypTypeInference,
    "Type Inference Failure",
    Severity::Error,
    false,
    "TYP",
    "049",
    TypSharedVariable,
    "Invalid Shared Variable",
    Severity::Error,
    false,
    "TYP",
    "050",
    TypLoopControlFlow,
    "Invalid Loop Control Flow",
    Severity::Error,
    false,
    "TYP",
    "051",
    TypRangeTypeMismatch,
    "Range Type Mismatch",
    Severity::Error,
    false,
    "TYP",
    "052",
    TypBuiltinConstructor,
    "Invalid Builtin Constructor",
    Severity::Error,
    false,
    "TYP",
    "053",
    TypVectorBuiltin,
    "Invalid Vector Builtin",
    Severity::Error,
    false,
    "TYP",
    "054",
    TypFunctionSignature,
    "Invalid Function Signature",
    Severity::Error,
    false,
    "TYP",
    "055",
    TypStaticMethodRestriction,
    "Static Method Restriction",
    Severity::Error,
    false,
    "TYP",
    "056",
    TypClassDefinition,
    "Invalid Class Definition",
    Severity::Error,
    false,
    "TYP",
    "057",
    TypTraitDefinition,
    "Invalid Trait Definition",
    Severity::Error,
    false,
    "TYP",
    "058",
    TypStructDefinition,
    "Invalid Struct Definition",
    Severity::Error,
    false,
    "TYP",
    "059",
    TypEnumDefinition,
    "Invalid Enum Definition",
    Severity::Error,
    false,
    "TYP",
    "060",
    TypAsyncAwait,
    "Invalid Async or Await Usage",
    Severity::Error,
    false,
    "TYP",
    "061",
    TypTypeInheritability,
    "Type Not Inheritable",
    Severity::Error,
    false,
    "TYP",
    "062",
    TypPatternMatch,
    "Invalid Pattern Match",
    Severity::Error,
    false,
    "TYP",
    "063",
    TypInvalidCast,
    "Invalid Cast",
    Severity::Error,
    false,
    "TYP",
    "064",
    TypShadowingNotAllowed,
    "Shadowing Not Allowed",
    Severity::Error,
    false,
    "TYP",
    "065",
    TypConstEvalArithmetic,
    "Invalid Constant Arithmetic",
    Severity::Error,
    false,
    "TYP",
    "066",
    TypOutParameterMisuse,
    "Out Parameter Misuse",
    Severity::Error,
    false,
    "TYP",
    "067",
    TypStringInterpolationType,
    "Type Not Valid in String Interpolation",
    Severity::Error,
    false,
    "TYP",
    "068",
    TypIntegerLiteralOutOfRange,
    "Integer Literal Out of Range",
    Severity::Error,
    false,
    // OWN — Ownership and resource management (1 code: 001)
    "OWN",
    "001",
    OwnResourceNotConsumedAtScopeExit,
    "Resource Not Consumed at Scope Exit",
    Severity::Warning,
    false,
    "OWN",
    "002",
    OwnLinearVariableNotConsumed,
    "Linear Variable Not Consumed",
    Severity::Error,
    false,
    "OWN",
    "003",
    OwnUseOfMovedValue,
    "Use of Moved Value",
    Severity::Error,
    false,
    "OWN",
    "004",
    OwnUnusedValue,
    "Unused Value",
    Severity::Error,
    false,
    // MIR — MIR lowering (15 codes: 001-015, with one reserved)
    "MIR",
    "001",
    MirUnsupportedExpression,
    "Unsupported Expression",
    Severity::Error,
    false,
    "MIR",
    "002",
    MirUnsupportedStatement,
    "Unsupported Statement",
    Severity::Error,
    false,
    "MIR",
    "003",
    MirUndefinedVariable,
    "Undefined Variable",
    Severity::Error,
    false,
    "MIR",
    "004",
    MirTypeNotFound,
    "Type Not Found",
    Severity::Error,
    false,
    "MIR",
    "005",
    MirBreakOutsideLoop,
    "Break Outside Loop",
    Severity::Error,
    false,
    "MIR",
    "006",
    MirContinueOutsideLoop,
    "Continue Outside Loop",
    Severity::Error,
    false,
    "MIR",
    "007",
    MirUnsupportedLeftHandSide,
    "Unsupported Left-Hand Side",
    Severity::Error,
    false,
    "MIR",
    "008",
    MirUnsupportedOperator,
    "Unsupported Operator",
    Severity::Error,
    false,
    "MIR",
    "009",
    MirUnsupportedRangeType,
    "Unsupported Range Type",
    Severity::Error,
    false,
    "MIR",
    "010",
    MirInvalidGpuLaunchArguments,
    "Invalid GPU Launch Arguments",
    Severity::Error,
    false,
    "MIR",
    "011",
    MirUnsupportedType,
    "Unsupported Type",
    Severity::Error,
    false,
    "MIR",
    "012",
    MirMissingStructField,
    "Missing Struct Field",
    Severity::Error,
    false,
    "MIR",
    "013",
    MirGpuLaunchMetadataMismatch,
    "GPU Launch Metadata Mismatch",
    Severity::Error,
    false,
    "MIR",
    "014",
    MirValidationFailed,
    "MIR Validation Failed",
    Severity::Error,
    false,
    "MIR",
    "015",
    MirCustomLoweringError,
    "Custom Lowering Error",
    Severity::Error,
    true,
    // CG — Code generation (8 codes: 001-008)
    "CG",
    "001",
    CgTargetIsaCreationFailed,
    "Target ISA Error",
    Severity::Error,
    false,
    "CG",
    "002",
    CgModuleCreationFailed,
    "Module Creation Error",
    Severity::Error,
    false,
    "CG",
    "003",
    CgFunctionDeclarationFailed,
    "Function Declaration Error",
    Severity::Error,
    false,
    "CG",
    "004",
    CgFunctionDefinitionFailed,
    "Function Definition Error",
    Severity::Error,
    false,
    "CG",
    "005",
    CgTranslationToBackendIrFailed,
    "Translation Error",
    Severity::Error,
    false,
    "CG",
    "006",
    CgObjectFileEmissionFailed,
    "Emit Error",
    Severity::Error,
    false,
    "CG",
    "007",
    CgBackendNotSupported,
    "Backend Not Supported",
    Severity::Error,
    false,
    "CG",
    "008",
    CgInternalCodegenError,
    "Internal Codegen Error",
    Severity::Error,
    false,
    // RT — Runtime (4 codes: 001-004)
    "RT",
    "001",
    RtDivisionByZero,
    "Division by Zero",
    Severity::Error,
    false,
    "RT",
    "002",
    RtRemainderByZero,
    "Remainder by Zero",
    Severity::Error,
    false,
    "RT",
    "003",
    RtIntegerOverflow,
    "Integer Overflow",
    Severity::Error,
    false,
    "RT",
    "004",
    RtInvalidOperand,
    "Invalid Operand",
    Severity::Error,
    false,
    // TAR — Target-specific capabilities (1 code: 001)
    "TAR",
    "001",
    TarShuffleOffsetTooLarge,
    "Shuffle Offset Exceeds Subgroup Size",
    Severity::Error,
    false,
    "TAR",
    "002",
    TarGpuCodeRestriction,
    "Unsupported Operation in GPU Code",
    Severity::Error,
    false,
    "TAR",
    "003",
    TarGpuDivModRange,
    "GPU Division or Modulo Range",
    Severity::Error,
    false,
    "TAR",
    "004",
    TarGpuBarrierControl,
    "Invalid GPU Barrier Control",
    Severity::Error,
    false,
    "TAR",
    "005",
    TarGpuParallelConstruct,
    "Invalid GPU Parallel Construct",
    Severity::Error,
    false,
    "TAR",
    "006",
    TarGpuResidencyViolation,
    "GPU Residency Violation",
    Severity::Error,
    false,
    "TAR",
    "007",
    TarGpuTypeNotAccelerable,
    "Type Not Accelerable",
    Severity::Error,
    false,
    "TAR",
    "008",
    TarGpuIncompatibleSignature,
    "GPU-Incompatible Function Signature",
    Severity::Error,
    false,
    "TAR",
    "009",
    TarGpuValueOutOfRange,
    "Value Out of Range for GPU Storage",
    Severity::Error,
    false,
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_all_codes_are_unique_by_wire_string() {
        let mut seen = std::collections::HashSet::new();
        for code in DiagnosticCode::all() {
            let wire = code.as_str();
            assert!(seen.insert(wire), "duplicate: {}", wire);
        }
    }

    #[test]
    fn test_area_extraction() {
        assert_eq!(DiagnosticCode::LexInvalidToken.area(), "LEX");
        assert_eq!(DiagnosticCode::ParUnexpectedToken.area(), "PAR");
        assert_eq!(
            DiagnosticCode::NamDeprecatedKernelContextIdentifier.area(),
            "NAM"
        );
        assert_eq!(DiagnosticCode::TypUnknownAttribute.area(), "TYP");
        assert_eq!(
            DiagnosticCode::OwnResourceNotConsumedAtScopeExit.area(),
            "OWN"
        );
        assert_eq!(DiagnosticCode::MirUnsupportedExpression.area(), "MIR");
        assert_eq!(DiagnosticCode::CgTargetIsaCreationFailed.area(), "CG");
        assert_eq!(DiagnosticCode::RtDivisionByZero.area(), "RT");
        assert_eq!(DiagnosticCode::TarShuffleOffsetTooLarge.area(), "TAR");
    }

    #[test]
    fn test_severity_extraction() {
        assert_eq!(DiagnosticCode::LexInvalidToken.severity(), Severity::Error);
        assert_eq!(
            DiagnosticCode::TypUnnecessaryDoubleNegation.severity(),
            Severity::Warning
        );
        assert_eq!(
            DiagnosticCode::OwnResourceNotConsumedAtScopeExit.severity(),
            Severity::Warning
        );
    }

    #[test]
    fn test_reserved_codes() {
        assert!(DiagnosticCode::LexUnclosedStringLiteral.is_reserved());
        assert!(!DiagnosticCode::LexInvalidToken.is_reserved());
    }

    #[test]
    fn test_wire_format() {
        assert_eq!(DiagnosticCode::LexInvalidToken.as_str(), "MER_LEX_001");
        assert_eq!(DiagnosticCode::TypTypeMismatch.as_str(), "MER_TYP_002");
        assert_eq!(
            DiagnosticCode::TarShuffleOffsetTooLarge.as_str(),
            "MER_TAR_001"
        );
    }

    #[test]
    fn test_from_str() {
        assert_eq!(
            "MER_LEX_001".parse::<DiagnosticCode>().unwrap(),
            DiagnosticCode::LexInvalidToken
        );
        assert_eq!(
            "MER_TYP_027".parse::<DiagnosticCode>().unwrap(),
            DiagnosticCode::TypDeprecatedAttribute
        );
        assert!("MER_INVALID_001".parse::<DiagnosticCode>().is_err());
    }
}
