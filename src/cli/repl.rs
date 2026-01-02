// SPDX-License-Identifier: Apache-2.0
// Copyright 2017–2026 Viacheslav Shynkarenko

use crate::error::compiler::CompilerError;
use crate::pipeline::Pipeline;
use rustyline::error::ReadlineError;
use rustyline::history::DefaultHistory;
use rustyline::Editor;

pub fn start() -> Result<(), CompilerError> {
    let mut rl = Editor::<(), DefaultHistory>::new()
        .map_err(|e| CompilerError::Internal(format!("Failed to initialize REPL editor: {}", e)))?;
    println!("Miri REPL. Type :quit to exit.");

    let pipeline = Pipeline::new();

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
                    _ => match pipeline.run(line) {
                        Ok(_) => println!("Execution successful."),
                        Err(e) => eprintln!("{}", e.report(line)),
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
