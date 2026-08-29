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

fn parse_miri_directive(info_string: &str) -> Option<MiriBlockDirective> {
    if !info_string.starts_with("miri") {
        return None;
    }

    let parts: Vec<&str> = info_string.split(',').map(|s| s.trim()).collect();
    if parts.is_empty() || parts[0] != "miri" {
        return None;
    }

    let mut directive = MiriBlockDirective {
        must_pass: true,
        fails_code: None,
    };

    for part in &parts[1..] {
        if let Some(code) = part.strip_prefix("fails=") {
            directive.must_pass = false;
            directive.fails_code = Some(code.to_string());
        } else if !part.is_empty() {
            // Unknown directive
            return None;
        }
    }

    Some(directive)
}

#[derive(Debug, Clone)]
struct MiriBlockDirective {
    must_pass: bool,
    fails_code: Option<String>,
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
        let expected_code = directive.fails_code.as_ref().unwrap();
        if parsed["ok"] != false {
            return Err(format!(
                "Line {}: Block must fail with {} but compiled clean\n\nCode:\n{}",
                block.line_number, expected_code, block.code
            ));
        }

        let diags = parsed["diagnostics"]
            .as_array()
            .ok_or_else(|| "diagnostics is not an array".to_string())?;

        let found = diags.iter().any(|d| {
            d["code"]
                .as_str()
                .map(|c| c == expected_code)
                .unwrap_or(false)
        });

        if !found {
            let codes: Vec<_> = diags.iter().filter_map(|d| d["code"].as_str()).collect();
            return Err(format!(
                "Line {}: Expected error code {} but got: {:?}\n\nCode:\n{}",
                block.line_number, expected_code, codes, block.code
            ));
        }
    }

    Ok(())
}

fn test_skill_block(block: &SkillBlock) -> Result<(), String> {
    let directive = match parse_miri_directive(&block.info_string) {
        Some(d) => d,
        None => {
            return Err(format!(
                "Line {}: Unknown directive in code block: {}",
                block.line_number, block.info_string
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
