// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use crate::utils::miri_cmd;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;

#[derive(Debug, Clone)]
struct SkillBlock {
    line_number: usize,
    info_string: String,
    code: String,
}

#[derive(Debug, Clone)]
struct SkillDirectives {
    name: Option<String>,
    description: Option<String>,
    blocks: Vec<SkillBlock>,
    body_line_count: usize,
}

fn parse_yaml_frontmatter(content: &str) -> (SkillDirectives, String) {
    let lines: Vec<&str> = content.lines().collect();
    let mut directives = SkillDirectives {
        name: None,
        description: None,
        blocks: Vec::new(),
        body_line_count: 0,
    };

    let mut idx = 0;

    // Skip initial `---`
    if idx < lines.len() && lines[idx].trim() == "---" {
        idx += 1;
    } else {
        return (directives, content.to_string());
    }

    // Parse YAML fields
    while idx < lines.len() && lines[idx].trim() != "---" {
        let line = lines[idx];
        if let Some(value) = line.strip_prefix("name:") {
            directives.name = Some(value.trim().trim_matches('"').to_string());
        } else if let Some(value) = line.strip_prefix("description:") {
            directives.description = Some(value.trim().trim_matches('"').to_string());
        }
        idx += 1;
    }

    // Skip closing `---`
    if idx < lines.len() && lines[idx].trim() == "---" {
        idx += 1;
    }

    let body_start = idx;

    // Count body lines
    directives.body_line_count = lines.len() - body_start;

    // Build body by joining the remaining lines (handles CRLF correctly by avoiding offset math)
    let body = lines[body_start..].join("\n");

    (directives, body)
}

fn extract_code_blocks(content: &str) -> Vec<SkillBlock> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = content.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        if line.starts_with("```") {
            let info_string = line[3..].trim().to_string();
            let line_number = i + 1;
            let mut code_lines = Vec::new();
            i += 1;

            while i < lines.len() && !lines[i].starts_with("```") {
                code_lines.push(lines[i]);
                i += 1;
            }

            if i < lines.len() && lines[i].starts_with("```") {
                let code = code_lines.join("\n");
                blocks.push(SkillBlock {
                    line_number,
                    info_string,
                    code,
                });
                i += 1;
            } else {
                break;
            }
        } else {
            i += 1;
        }
    }

    blocks
}

fn parse_miri_directive(info_string: &str) -> Result<MiriBlockDirective, String> {
    if !info_string.starts_with("miri") {
        return Err("Expected 'miri' directive".to_string());
    }

    // Check if expects-message= is present. It must be the last directive in the info string.
    // We check this by ensuring no comma appears after expects-message=.
    let has_expects_message = info_string.contains("expects-message=");
    if has_expects_message {
        if let Some(idx) = info_string.find("expects-message=") {
            // Everything after expects-message= should be the message text (trimmed).
            // If there's a comma after expects-message=, it means the directive was not last.
            let after_prefix = &info_string[idx + "expects-message=".len()..];
            if after_prefix.trim().is_empty() {
                return Err("expects-message= requires a non-empty message value".to_string());
            }
            // Check for directive keywords that should not appear in the message.
            // Use comma-prefixed checks since directive keywords always follow a comma separator.
            let msg_text = after_prefix.trim();
            if msg_text.contains(",fails=") || msg_text.contains(",expects-message=") {
                return Err(
                    "expects-message= must be the last directive; no other directives may appear after it"
                        .to_string(),
                );
            }
        }
    }

    // Extract the main directives (before expects-message=, if present).
    let directive_part = if let Some(idx) = info_string.find("expects-message=") {
        info_string[..idx].trim_end_matches(',')
    } else {
        info_string
    };

    let parts: Vec<&str> = directive_part.split(',').map(|s| s.trim()).collect();
    if parts.is_empty() || parts[0] != "miri" {
        return Err("Invalid directive format".to_string());
    }

    let mut directive = MiriBlockDirective {
        must_pass: true,
        fails_code: None,
        expects_message: None,
    };

    for part in &parts[1..] {
        if let Some(code) = part.strip_prefix("fails=") {
            if directive.fails_code.is_some() {
                return Err(
                    "fails= directive appears more than once; only one error code per block"
                        .to_string(),
                );
            }
            directive.must_pass = false;
            directive.fails_code = Some(code.to_string());
        } else if !part.is_empty() {
            // Unknown directive
            return Err(format!("Unknown directive: {}", part));
        }
    }

    // Extract expects-message if present
    if has_expects_message {
        if let Some(idx) = info_string.find("expects-message=") {
            let message_part = info_string[idx + "expects-message=".len()..].to_string();
            directive.expects_message = Some(message_part.trim().to_string());
        }
    }

    // Validate: expects-message= is only meaningful with fails=
    if directive.expects_message.is_some() && directive.must_pass {
        return Err(
            "expects-message= can only be used with fails=; it pins the error message".to_string(),
        );
    }

    Ok(directive)
}

#[derive(Debug, Clone)]
struct MiriBlockDirective {
    must_pass: bool,
    fails_code: Option<String>,
    expects_message: Option<String>,
}

fn run_miri_check(block: &SkillBlock) -> Result<serde_json::Value, String> {
    let mut file =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    write!(file, "{}", block.code).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let test_path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    cmd.env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env("RUST_BACKTRACE", "0")
        .arg("check")
        .arg(&test_path)
        .arg("--format")
        .arg("json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run miri: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON output: {}", e))
}

fn run_miri_build(block: &SkillBlock) -> Result<serde_json::Value, String> {
    let mut file =
        NamedTempFile::new().map_err(|e| format!("Failed to create temp file: {}", e))?;
    write!(file, "{}", block.code).map_err(|e| format!("Failed to write temp file: {}", e))?;

    let test_path = file.path().to_str().unwrap().to_string();

    let mut cmd = miri_cmd();
    let stdlib_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("stdlib");

    cmd.env("MIRI_STDLIB_PATH", stdlib_path.to_str().unwrap())
        .env("RUST_BACKTRACE", "0")
        .arg("build")
        .arg(&test_path)
        .arg("--format")
        .arg("json");

    let output = cmd
        .output()
        .map_err(|e| format!("Failed to run miri: {}", e))?;

    let stdout = String::from_utf8_lossy(&output.stdout);
    serde_json::from_str(&stdout).map_err(|e| format!("Failed to parse JSON output: {}", e))
}

fn validate_skill_block_result(
    block: &SkillBlock,
    directive: &MiriBlockDirective,
    parsed: &serde_json::Value,
) -> Result<(), String> {
    if directive.must_pass {
        if parsed["ok"] != true {
            let empty_vec = vec![];
            let diags_array = parsed["diagnostics"].as_array().unwrap_or(&empty_vec);
            let codes: Vec<_> = diags_array
                .iter()
                .filter_map(|d| d["code"].as_str())
                .collect();
            return Err(format!(
                "Line {}: Block must pass but got errors: {:?}\n\nCode:\n{}",
                block.line_number, codes, block.code
            ));
        }
    } else {
        // Safe unwraps: validate_miri_blocks() ensures every fails= block has expects_message= set
        let expected_code = directive.fails_code.as_ref().unwrap();
        let expected_message = directive.expects_message.as_ref().unwrap();
        if parsed["ok"] != false {
            return Err(format!(
                "Line {}: Block must fail with {} but compiled clean\n\nCode:\n{}",
                block.line_number, expected_code, block.code
            ));
        }

        let diags = parsed["diagnostics"]
            .as_array()
            .ok_or_else(|| "diagnostics is not an array".to_string())?;

        // Find the diagnostic with the expected code and verify its message contains expected_message
        let matching_diag = diags.iter().find(|d| {
            d["code"]
                .as_str()
                .map(|c| c == expected_code)
                .unwrap_or(false)
        });

        match matching_diag {
            None => {
                let codes: Vec<_> = diags.iter().filter_map(|d| d["code"].as_str()).collect();
                return Err(format!(
                    "Line {}: Expected error code {} but got: {:?}\n\nCode:\n{}",
                    block.line_number, expected_code, codes, block.code
                ));
            }
            Some(diag) => {
                let message = diag["message"].as_str().unwrap_or("<no message>");
                if !message.contains(expected_message) {
                    return Err(format!(
                        "Line {}: Error code {} found, but message does not contain expected substring.\n\
                         Expected substring: {}\n\
                         Actual message: {}\n\nCode:\n{}",
                        block.line_number, expected_code, expected_message, message, block.code
                    ));
                }
            }
        }
    }

    Ok(())
}

fn test_skill_block(block: &SkillBlock) -> Result<(), String> {
    let directive = match parse_miri_directive(&block.info_string) {
        Ok(d) => d,
        Err(e) => {
            return Err(format!(
                "Line {}: Invalid directive in code block: {}\n  Info: {}",
                block.line_number, e, block.info_string
            ))
        }
    };

    let parsed = if directive.must_pass {
        run_miri_build(block)?
    } else {
        run_miri_check(block)?
    };
    validate_skill_block_result(block, &directive, &parsed)
}

fn validate_skill_frontmatter(
    skill_dir: &Path,
    directives: &SkillDirectives,
) -> Result<(), String> {
    let dir_name = skill_dir
        .file_name()
        .ok_or_else(|| "Cannot get directory name".to_string())?
        .to_string_lossy()
        .to_string();

    if let Some(name) = &directives.name {
        if name != &dir_name {
            return Err(format!(
                "Skill {}: name in frontmatter '{}' does not match directory name '{}'",
                skill_dir.display(),
                name,
                dir_name
            ));
        }
    } else {
        return Err(format!(
            "Skill {} missing name in frontmatter",
            skill_dir.display()
        ));
    }

    if directives
        .description
        .as_ref()
        .map_or(true, |d| d.is_empty())
    {
        return Err(format!(
            "Skill {} missing or empty description in frontmatter",
            skill_dir.display()
        ));
    }

    if directives.body_line_count > 400 {
        return Err(format!(
            "Skill {} body exceeds 400 lines: {} lines",
            skill_dir.display(),
            directives.body_line_count
        ));
    }

    Ok(())
}

fn validate_miri_blocks(skill_dir: &Path, miri_blocks: &[&SkillBlock]) -> Result<(), String> {
    if miri_blocks.is_empty() {
        return Err(format!(
            "Skill {} has zero miri code blocks",
            skill_dir.display()
        ));
    }

    let has_fails_block = miri_blocks.iter().any(|b| b.info_string.contains("fails="));
    if !has_fails_block {
        return Err(format!(
            "Skill {} has zero fails= blocks (all blocks must test anti-hallucination)",
            skill_dir.display()
        ));
    }

    // Every fails= block must also carry expects-message=
    for block in miri_blocks {
        if block.info_string.contains("fails=") && !block.info_string.contains("expects-message=") {
            return Err(format!(
                "Skill {} line {}: fails= block must also carry expects-message= to pin the diagnostic message",
                skill_dir.display(),
                block.line_number
            ));
        }
    }

    Ok(())
}

fn test_skill(skill_dir: &Path) -> Result<(), String> {
    let skill_md = skill_dir.join("SKILL.md");
    if !skill_md.exists() {
        return Err(format!("Skill {} missing SKILL.md", skill_dir.display()));
    }

    let content = fs::read_to_string(&skill_md)
        .map_err(|e| format!("Failed to read {}: {}", skill_md.display(), e))?;

    let (mut directives, body) = parse_yaml_frontmatter(&content);
    directives.blocks = extract_code_blocks(&body);

    validate_skill_frontmatter(skill_dir, &directives)?;

    let miri_blocks: Vec<_> = directives
        .blocks
        .iter()
        .filter(|b| b.info_string.starts_with("miri"))
        .collect();

    validate_miri_blocks(skill_dir, &miri_blocks)?;

    for (i, block) in miri_blocks.iter().enumerate() {
        if let Err(e) = test_skill_block(block) {
            eprintln!(
                "Block #{} at line {}: {}\nCode:\n{}",
                i, block.line_number, e, block.code
            );
            return Err(e);
        }
    }

    Ok(())
}

#[test]
fn test_skills() {
    let skills_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("skills");

    if !skills_dir.exists() {
        panic!("skills/ directory not found at {}", skills_dir.display());
    }

    let mut skill_dirs: Vec<_> = fs::read_dir(&skills_dir)
        .expect("Failed to read skills directory")
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let path = entry.path();
            if path.is_dir() {
                Some(path)
            } else {
                None
            }
        })
        .collect();

    skill_dirs.sort();

    if skill_dirs.is_empty() {
        panic!("No skill directories found under skills/");
    }

    let mut errors = Vec::new();

    for skill_dir in skill_dirs {
        match test_skill(&skill_dir) {
            Ok(()) => println!("✓ {}", skill_dir.display()),
            Err(e) => {
                errors.push(format!("{}: {}", skill_dir.display(), e));
            }
        }
    }

    if !errors.is_empty() {
        panic!("Skill validation failed:\n{}", errors.join("\n"));
    }
}

#[test]
fn test_directive_parsing_valid() {
    // Valid: miri with must-pass
    assert!(parse_miri_directive("miri").is_ok());

    // Valid: miri with fails= and expects-message=
    let result = parse_miri_directive("miri,fails=MER_TYP_001,expects-message=error text");
    assert!(result.is_ok());
    let dir = result.unwrap();
    assert!(!dir.must_pass);
    assert_eq!(dir.fails_code, Some("MER_TYP_001".to_string()));
    assert_eq!(dir.expects_message, Some("error text".to_string()));

    // Valid: with message containing spaces and punctuation
    let result = parse_miri_directive(
        "miri,fails=MER_PAR_001,expects-message=Expected an expression, but found :",
    );
    assert!(result.is_ok());
    let dir = result.unwrap();
    assert_eq!(
        dir.expects_message,
        Some("Expected an expression, but found :".to_string())
    );
}

#[test]
fn test_directive_mis_ordered_fails_before_expects_message() {
    // Invalid: fails= appears after expects-message= (expects-message must be last)
    let result = parse_miri_directive("miri,expects-message=foo,fails=MER_TYP_034");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("must be the last directive"));
}

#[test]
fn test_directive_expects_message_without_fails() {
    // Invalid: expects-message= only makes sense with fails=
    let result = parse_miri_directive("miri,expects-message=some error");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("can only be used with fails="));
}

#[test]
fn test_directive_expects_message_empty() {
    // Invalid: expects-message= must have a non-empty value
    let result = parse_miri_directive("miri,fails=MER_TYP_001,expects-message=");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("non-empty"));
}

#[test]
fn test_directive_comma_prefixed_keyword_detection() {
    // E. Precise check: comma-prefixed keywords only. Message legitimately containing "fails="
    // (without a comma prefix) is allowed. Real scenario: message mentions "fails=" in an explanation.
    let result = parse_miri_directive(
        "miri,fails=MER_TYP_001,expects-message=This error occurs when you use fails= in the wrong place",
    );
    assert!(
        result.is_ok(),
        "Message containing bare 'fails=' should be allowed"
    );

    // But comma-prefixed directive after expects-message= is still rejected
    let result = parse_miri_directive("miri,expects-message=foo,fails=MER_TYP_034");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must be the last directive"));
}

#[test]
fn test_directive_duplicate_fails() {
    // F. Reject duplicate fails= directives
    let result = parse_miri_directive("miri,fails=MER_TYP_001,fails=MER_TYP_002");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("appears more than once"),
        "Expected 'appears more than once' in error, got: {}",
        err
    );
}
