// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Explanation of diagnostic codes from embedded markdown documentation.
//!
//! This module parses the embedded diagnostic documentation and provides
//! a structured explanation for each code. The parser is infallible: missing
//! sections yield None or empty strings rather than errors, allowing `miri explain`
//! to exit 0 on any registered code.

use crate::diagnostics::json::JsonExplanation;
use crate::diagnostics::DiagnosticCode;

/// A structured explanation of a diagnostic code extracted from markdown documentation.
#[derive(Debug, Clone)]
pub struct Explanation {
    /// The diagnostic code this explanation documents.
    pub code: DiagnosticCode,
    /// The "Rule" section explaining the check or error.
    pub rule: String,
    /// The "Before" example (live codes only, absent for reserved codes).
    pub example_before: Option<String>,
    /// The "After" example (live codes only, absent for reserved codes).
    pub example_after: Option<String>,
    /// The relative path to the reference documentation.
    pub reference: Option<String>,
}

impl Explanation {
    /// Parse an explanation from the embedded markdown for the given diagnostic code.
    ///
    /// The parser is infallible. Missing sections yield empty strings or None,
    /// rather than failing.
    pub fn parse(code: DiagnosticCode, doc: &str) -> Self {
        let rule = extract_section(doc, "Rule");
        let example_before = extract_section_opt(doc, "Before").map(|s| strip_code_fence(&s));
        let example_after = extract_section_opt(doc, "After").map(|s| strip_code_fence(&s));
        let reference = extract_reference_link(doc);

        Self {
            code,
            rule,
            example_before,
            example_after,
            reference,
        }
    }

    /// Convert to the serializable form carried by the JSON envelope.
    ///
    /// Title, severity and retirement status come from the registry rather than
    /// from the documentation text, so the two can never drift apart.
    pub fn to_json(&self) -> JsonExplanation {
        JsonExplanation {
            code: self.code.as_str().to_string(),
            title: self.code.title().to_string(),
            severity: self.code.severity().as_str().to_string(),
            reserved: self.code.is_reserved(),
            rule: self.rule.clone(),
            example_before: self.example_before.clone(),
            example_after: self.example_after.clone(),
            reference: self.reference.clone(),
        }
    }
}

/// Extract a section by its `## Heading` and return the content.
/// If the section is not found or is empty, returns an empty string.
fn extract_section(doc: &str, heading: &str) -> String {
    let section = extract_section_opt(doc, heading);
    section.unwrap_or_default()
}

/// Extract a section by its `## Heading` and return Some(content) if found and non-empty,
/// or None otherwise.
fn extract_section_opt(doc: &str, heading: &str) -> Option<String> {
    let marker = format!("## {}", heading);
    let start = doc.find(&marker)?;

    let content_start = start + marker.len();
    let after_heading = &doc[content_start..];

    let next_heading = after_heading.find("## ");
    let end = next_heading
        .map(|pos| content_start + pos)
        .unwrap_or(doc.len());

    let content = doc[content_start..end].trim();
    if content.is_empty() {
        None
    } else {
        Some(content.to_string())
    }
}

/// Unwrap a fenced code block, yielding the source it contains.
///
/// A consumer of an example wants the program, not the markdown that presents
/// it. Text that is not a single fenced block is returned unchanged, which is
/// what keeps the codes whose example is prose rather than source readable.
fn strip_code_fence(section: &str) -> String {
    let trimmed = section.trim();
    let Some(after_open) = trimmed.strip_prefix("```") else {
        return trimmed.to_string();
    };
    let Some((language_line, body)) = after_open.split_once('\n') else {
        return trimmed.to_string();
    };
    // A fence tagged with a language opens a block; a bare ``` on that line
    // would mean the section is something other than one code block.
    if language_line.contains("```") {
        return trimmed.to_string();
    }
    match body.rfind("```") {
        Some(close) => body[..close].trim_end().to_string(),
        None => trimmed.to_string(),
    }
}

/// Extract the relative path from a markdown link in the Reference section.
/// Looks for patterns like `[text](../reference/x.md)` or `[../reference/x.md](../reference/x.md)`.
fn extract_reference_link(doc: &str) -> Option<String> {
    let section = extract_section_opt(doc, "Reference")?;

    for line in section.lines() {
        if let Some(start) = line.find("](") {
            if let Some(end) = line[start + 2..].find(")") {
                let path = &line[start + 2..start + 2 + end];
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_section_simple() {
        let doc = "## Rule\n\nThis is the rule.\n\n## Reference\n\nSee something.";
        let rule = extract_section(doc, "Rule");
        assert_eq!(rule, "This is the rule.");
    }

    #[test]
    fn test_extract_section_missing() {
        let doc = "## Rule\n\nThis is the rule.";
        let ref_section = extract_section(doc, "Reference");
        assert_eq!(ref_section, "");
    }

    #[test]
    fn test_extract_reference_link_simple() {
        let doc = "## Reference\n\nSee [the documentation](../reference/types.md).";
        let link = extract_reference_link(doc);
        assert_eq!(link, Some("../reference/types.md".to_string()));
    }

    #[test]
    fn test_extract_reference_link_mirror_pattern() {
        let doc = "## Reference\n\n[../reference/types.md](../reference/types.md)";
        let link = extract_reference_link(doc);
        assert_eq!(link, Some("../reference/types.md".to_string()));
    }

    #[test]
    fn test_extract_reference_link_missing() {
        let doc = "## Reference\n\nNo link here.";
        let link = extract_reference_link(doc);
        assert_eq!(link, None);
    }
}
