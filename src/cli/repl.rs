// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::error::compiler::CompilerError;
use crate::interpreter::Value;
use crate::pipeline::Pipeline;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;

/// REPL session that maintains persistent context across lines.
struct ReplSession {
    /// Accumulated statements that persist (declarations AND assignments)
    statements: Vec<String>,
    pipeline: Pipeline,
}

impl ReplSession {
    fn new() -> Self {
        Self {
            statements: Vec::new(),
            pipeline: Pipeline::new(),
        }
    }

    /// Check if a line is an expression that should return a value.
    fn is_display_expression(line: &str) -> bool {
        let trimmed = line.trim();
        // Simple identifiers or expressions that aren't assignments/declarations
        !trimmed.starts_with("let ")
            && !trimmed.starts_with("var ")
            && !trimmed.starts_with("fn ")
            && !trimmed.starts_with("struct ")
            && !trimmed.starts_with("enum ")
            && !trimmed.starts_with("type ")
            && !trimmed.contains(" = ") // Assignments like x = 10
    }

    /// Evaluate a line in the REPL context.
    /// All statements are accumulated for persistent state.
    fn eval(&mut self, line: &str) -> Result<Option<Value>, CompilerError> {
        let should_display = Self::is_display_expression(line);

        // Build the full source with accumulated statements + current line
        let mut full_source = self.statements.join("\n");
        if !full_source.is_empty() {
            full_source.push('\n');
        }
        full_source.push_str(line);

        // Try to interpret
        match self.pipeline.interpret(&full_source) {
            Ok(value) => {
                // Success! Persist ALL statements
                self.statements.push(line.to_string());

                // Only show value for display expressions (not declarations/assignments)
                if should_display && !matches!(value, Value::None) {
                    Ok(Some(value))
                } else {
                    Ok(None)
                }
            }
            Err(e) => Err(e),
        }
    }
}

pub fn start() -> Result<(), CompilerError> {
    let mut rl = Editor::<(), DefaultHistory>::new()
        .map_err(|e| CompilerError::Internal(format!("Failed to initialize REPL editor: {}", e)))?;

    // Print version string like Python
    println!("Miri {}", crate::cli::version::version_string());
    println!("Type :quit, :q or :exit to exit. Type :clear or :reset to clear context.");

    let mut session = ReplSession::new();

    loop {
        let readline = rl.readline("miri> ");
        match readline {
            Ok(line) => {
                let line = line.trim();
                if line.is_empty() {
                    continue;
                }
                let _ = rl.add_history_entry(line);

                match line {
                    ":quit" | ":q" | ":exit" => break,
                    ":clear" | ":reset" => {
                        session = ReplSession::new();
                        println!("Context cleared.");
                    }
                    _ => match session.eval(line) {
                        Ok(Some(value)) => {
                            // Show result for expressions
                            println!("=> {}", value);
                        }
                        Ok(None) => {
                            // Statement stored, no output needed
                        }
                        Err(e) => {
                            // Build source for error reporting
                            let full_source =
                                format!("{}\n{}", session.statements.join("\n"), line);
                            eprintln!("{}", e.report(&full_source));
                        }
                    },
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!("CTRL-C");
                break;
            }
            Err(ReadlineError::Eof) => {
                println!("CTRL-D");
                break;
            }
            Err(err) => {
                println!("Error: {:?}", err);
                break;
            }
        }
    }

    Ok(())
}
