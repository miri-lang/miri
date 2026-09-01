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
    /// The title of the referenced page (from `# Heading`).
    pub reference_title: Option<String>,
    /// The lead paragraph of the referenced page.
    pub reference_summary: Option<String>,
    /// Message shapes that this code can emit (backticked, from `## Messages` section).
    pub messages: Vec<String>,
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
        let messages = extract_messages(doc);

        // Extract reference title and summary if a reference path exists
        let (reference_title, reference_summary) = reference
            .as_ref()
            .and_then(|path| get_embedded_reference(path))
            .map(extract_reference_title_and_summary)
            .unwrap_or((None, None));

        Self {
            code,
            rule,
            example_before,
            example_after,
            reference,
            reference_title,
            reference_summary,
            messages,
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
            reference_title: self.reference_title.clone(),
            reference_summary: self.reference_summary.clone(),
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

/// Extract all markdown links from the Reference section.
/// Looks for patterns like `[text](../reference/x.md)` or `[../reference/x.md](../reference/x.md)`.
/// Returns all link destinations found, in order.
#[allow(dead_code)]
fn extract_all_reference_links(doc: &str) -> Vec<String> {
    let section = match extract_section_opt(doc, "Reference") {
        Some(s) => s,
        None => return vec![],
    };

    let mut links = Vec::new();
    for line in section.lines() {
        for (idx, _) in line.match_indices("](") {
            if let Some(end_pos) = line[idx + 2..].find(")") {
                let path = &line[idx + 2..idx + 2 + end_pos];
                if !path.is_empty() {
                    links.push(path.to_string());
                }
            }
        }
    }
    links
}

/// Extract the first markdown link from the Reference section (for backward compatibility).
/// Looks for patterns like `[text](../reference/x.md)` or `[../reference/x.md](../reference/x.md)`.
fn extract_reference_link(doc: &str) -> Option<String> {
    let section = extract_section_opt(doc, "Reference")?;
    for line in section.lines() {
        for (idx, _) in line.match_indices("](") {
            if let Some(end_pos) = line[idx + 2..].find(")") {
                let path = &line[idx + 2..idx + 2 + end_pos];
                if !path.is_empty() {
                    return Some(path.to_string());
                }
            }
        }
    }
    None
}

/// Extract message shapes from the `## Messages` section.
/// The section is a bullet list with backticked shapes: `- \`Unknown type: {name}\``.
/// Each backticked shape is extracted and returned in order.
///
/// Uses CommonMark code-span rules: a span opens with N backticks and closes at
/// exactly N backticks. If both the first and last character of the content are
/// spaces, a single space is removed from each end (CommonMark spec).
/// Empty shapes are silently skipped (never added to the list).
fn extract_messages(doc: &str) -> Vec<String> {
    let section = match extract_section_opt(doc, "Messages") {
        Some(s) => s,
        None => return vec![],
    };

    let mut messages = vec![];
    for line in section.lines() {
        let trimmed = line.trim();
        if let Some(after_dash) = trimmed.strip_prefix('-') {
            let after_dash = after_dash.trim();
            // Look for a code span: one or more backticks, followed by content,
            // followed by the same number of backticks.
            if let Some(shape) = extract_code_span(after_dash) {
                // Only push non-empty shapes.
                if !shape.is_empty() {
                    messages.push(shape);
                }
            }
        }
    }
    messages
}

/// Extract a CommonMark code span from the beginning of a string.
/// Returns the content inside the backticks (with space trimming applied),
/// or None if no valid code span is found.
fn extract_code_span(text: &str) -> Option<String> {
    // Count leading backticks.
    let mut backtick_count = 0;
    for ch in text.chars() {
        if ch == '`' {
            backtick_count += 1;
        } else {
            break;
        }
    }

    // If no leading backticks, no code span.
    if backtick_count == 0 {
        return None;
    }

    // The content starts after the opening backticks.
    let content_start = backtick_count;
    let rest = &text[content_start..];

    // Look for the closing backtick sequence of the same length.
    // Iterate via char_indices to stay at valid character boundaries.
    for (pos, _) in rest.char_indices() {
        if rest[pos..].starts_with(&"`".repeat(backtick_count)) {
            // Check that this is a valid close (not followed by more backticks).
            let close_end = pos + backtick_count;
            let is_valid_close = close_end >= rest.len() || rest.as_bytes()[close_end] != b'`';
            if is_valid_close {
                // Found the closing backticks.
                let mut content = rest[..pos].to_string();
                // Apply CommonMark space trimming: if content starts and ends with a space,
                // remove one space from each end.
                if content.starts_with(' ') && content.ends_with(' ') && content.len() > 1 {
                    content.remove(0);
                    content.pop();
                }
                return Some(content);
            }
        }
    }

    // No closing backticks found.
    None
}

/// Embedded reference pages (relative path -> markdown content).
const EMBEDDED_REFERENCES: &[(&str, &str)] = &[
    (
        "../reference/build.md",
        include_str!("../../docs/reference/build.md"),
    ),
    (
        "../reference/codegen.md",
        include_str!("../../docs/reference/codegen.md"),
    ),
    (
        "../reference/imports.md",
        include_str!("../../docs/reference/imports.md"),
    ),
    (
        "../reference/lexer.md",
        include_str!("../../docs/reference/lexer.md"),
    ),
    (
        "../reference/mir.md",
        include_str!("../../docs/reference/mir.md"),
    ),
    (
        "../reference/naming.md",
        include_str!("../../docs/reference/naming.md"),
    ),
    (
        "../reference/ownership.md",
        include_str!("../../docs/reference/ownership.md"),
    ),
    (
        "../reference/parser.md",
        include_str!("../../docs/reference/parser.md"),
    ),
    (
        "../reference/runtime.md",
        include_str!("../../docs/reference/runtime.md"),
    ),
    (
        "../reference/targets.md",
        include_str!("../../docs/reference/targets.md"),
    ),
    (
        "../reference/types.md",
        include_str!("../../docs/reference/types.md"),
    ),
];

/// Get the content of an embedded reference page by its relative path.
fn get_embedded_reference(path: &str) -> Option<&'static str> {
    EMBEDDED_REFERENCES
        .iter()
        .find(|(ref_path, _)| ref_path == &path)
        .map(|(_, content)| *content)
}

/// Extract the title (first `# ` line) and lead paragraph from reference markdown.
/// Returns (title, summary) as Options. Both are Some only if both are found.
fn extract_reference_title_and_summary(content: &str) -> (Option<String>, Option<String>) {
    let mut title = None;
    let mut summary = String::new();
    let mut in_summary = false;

    for line in content.lines() {
        // Extract title from first `# ` line
        if title.is_none() && line.starts_with("# ") {
            title = Some(line[2..].trim().to_string());
            in_summary = true;
            continue;
        }

        // If we found a title, collect lines until we hit `## ` (section heading)
        if in_summary {
            if line.starts_with("## ") {
                break;
            }
            // Skip empty lines at the start of the summary
            if !summary.is_empty() || !line.trim().is_empty() {
                if !summary.is_empty() {
                    summary.push('\n');
                }
                summary.push_str(line);
            }
        }
    }

    let summary = summary.trim().to_string();
    let summary = if summary.is_empty() {
        None
    } else {
        Some(summary)
    };
    (title, summary)
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

    #[test]
    fn test_extract_messages_simple() {
        let doc =
            "## Messages\n\n- `Unknown type: {name}`\n- `Unknown type '{name}' in declaration`";
        let messages = extract_messages(doc);
        assert_eq!(
            messages,
            vec![
                "Unknown type: {name}".to_string(),
                "Unknown type '{name}' in declaration".to_string()
            ]
        );
    }

    #[test]
    fn test_extract_messages_missing() {
        let doc = "## Rule\n\nSome rule.";
        let messages = extract_messages(doc);
        assert!(messages.is_empty());
    }

    #[test]
    fn test_embedded_reference_pages_have_title_and_summary() {
        for (path, content) in EMBEDDED_REFERENCES {
            let (title, summary) = extract_reference_title_and_summary(content);
            assert!(
                title.is_some() && !title.as_ref().unwrap().is_empty(),
                "Reference page {} must have a non-empty title (# Heading)",
                path
            );
            assert!(
                summary.is_some() && !summary.as_ref().unwrap().is_empty(),
                "Reference page {} must have a non-empty lead paragraph before the first ## section",
                path
            );
        }
    }

    #[test]
    fn test_reference_links_resolve_to_embedded_pages() {
        use crate::diagnostics::DiagnosticCode;

        for code in DiagnosticCode::all() {
            let doc = code.doc();
            let all_links = extract_all_reference_links(&doc);

            for link in all_links {
                assert!(
                    get_embedded_reference(&link).is_some(),
                    "Code {}: Reference link '{}' does not resolve to an embedded page",
                    code.as_str(),
                    link
                );
            }
        }
    }

    #[test]
    fn test_extract_code_span_single_backtick() {
        let input = "`hello world`";
        let result = extract_code_span(input);
        assert_eq!(result, Some("hello world".to_string()));
    }

    #[test]
    fn test_extract_code_span_double_backtick_with_inner_single() {
        let input = "``hello `code` world``";
        let result = extract_code_span(input);
        assert_eq!(result, Some("hello `code` world".to_string()));
    }

    #[test]
    fn test_extract_code_span_space_trimming_both_sides() {
        let input = "` content with spaces `";
        let result = extract_code_span(input);
        assert_eq!(result, Some("content with spaces".to_string()));
    }

    #[test]
    fn test_extract_code_span_space_trimming_only_one_side() {
        let input = "`content `";
        let result = extract_code_span(input);
        // Only trailing space, no leading space, so no trimming
        assert_eq!(result, Some("content ".to_string()));
    }

    #[test]
    fn test_extract_code_span_no_closing_backticks() {
        let input = "`unclosed";
        let result = extract_code_span(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_code_span_no_leading_backtick() {
        let input = "hello world";
        let result = extract_code_span(input);
        assert_eq!(result, None);
    }

    #[test]
    fn test_extract_code_span_multibyte_character_inside() {
        // Test with em dash (U+2014, 3-byte UTF-8)
        let input = "`—message`";
        let result = extract_code_span(input);
        assert_eq!(result, Some("—message".to_string()));
    }

    #[test]
    fn test_extract_code_span_multibyte_character_after_opening() {
        // Test with em dash immediately after opening backtick
        let input = "`—after`";
        let result = extract_code_span(input);
        assert_eq!(result, Some("—after".to_string()));
    }

    #[test]
    fn test_extract_code_span_multiple_multibyte_characters() {
        // Test with multiple multi-byte characters and quotes
        let input = "`café with quotes`";
        let result = extract_code_span(input);
        assert_eq!(result, Some("café with quotes".to_string()));
    }
}
