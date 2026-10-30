// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The line map a build writes as `<executable>.lines`: the source line each
//! run of every compiled function's machine code was generated from, for a
//! profiler to attribute a sampled address to a line.
//!
//! ```text
//! miri-lines 2
//! file 0 prog.mi
//! file 1 system/math.mi
//! fn 0 miri.mix_all size=212
//!   24 40 0:8
//!   40 64 1:60 <- 0:8
//! ```
//!
//! After the version line comes the file table, one `file <index> <name>`
//! line per source in [`SourceId`] order, the name running to the end of the
//! line: the program by its file name (`<program>` for a build given no
//! file), a library module by its file under the library root
//! (`system/json.mi`), a module of the program's own by its file under the
//! program's directory marked `local:` (`local:util/strings.mi`), never by
//! where any of them sits on the machine that built it. A control character
//! or a backslash in a name is written as its Rust escape (`\n`, `\\`), so
//! a name is always one line. Then one `fn <index> <link symbol>
//! size=<bytes>` line per function, in the order the backend was handed them, each
//! followed by its runs as `<start> <end> <file>:<line>`: byte offsets from
//! the function's first byte, sorted, a run reaching the next one on the same
//! line, inlined through the same calls, merged into it. Code inlined into
//! the function from another one lists, after its own line, each call it was
//! inlined through as ` <- <file>:<line>`, innermost first: the line calling
//! the function the code was written in, then the line calling the function
//! that call was written in, out to a line of the function itself. The runs
//! of a function compiled from source cover every byte of its code, the
//! code the backend left unlocated included: its frame setup is on the line
//! its signature is written on. A function the compiler synthesized with
//! no source lists no runs, and so does each function the backend generated
//! itself, a drop thunk or a closure trampoline, listed after the rest: a
//! profiler charges the time spent in one to the line that called it. Lines are
//! 1-based and count `\n` bytes, as diagnostics do.

use crate::codegen::FunctionCodeMap;
use crate::error::{CompilerError, SourceId};
use std::fmt;

/// The version the first line of a line map names.
const FORMAT_VERSION: u32 = 2;

/// The name the file table gives a program the build was handed as text,
/// with no file of its own.
pub(crate) const UNNAMED_PROGRAM: &str = "<program>";

/// One source of the program, as the line map names it and reads its lines.
pub(crate) struct SourceFile<'s> {
    pub name: &'s str,
    pub text: &'s str,
}

/// The line map of a program's compiled functions.
pub(crate) struct LineMap<'m> {
    /// Every source, indexed by [`SourceId`].
    files: &'m [SourceFile<'m>],
    /// Every compiled function, in the order the backend was handed it.
    functions: &'m [FunctionCodeMap],
}

impl<'m> LineMap<'m> {
    /// The line map of `functions`, read against `files`. A program always
    /// compiles at least its entry point, so no function at all means the
    /// backend recorded no code map, and the map is refused rather than
    /// written empty.
    pub(crate) fn new(
        files: &'m [SourceFile<'m>],
        functions: &'m [FunctionCodeMap],
    ) -> Result<Self, CompilerError> {
        if functions.is_empty() {
            return Err(CompilerError::Codegen(
                "`--emit lines` needs a backend that records where its code came from, \
                 and the selected CPU backend recorded none"
                    .to_string(),
            ));
        }
        Ok(Self { files, functions })
    }
}

/// A file name as the file table writes it: a control character or a
/// backslash escaped, so the name stays on its line.
struct FileName<'n>(&'n str);

impl fmt::Display for FileName<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for character in self.0.chars() {
            if character.is_control() || character == '\\' {
                write!(f, "{}", character.escape_default())?;
            } else {
                write!(f, "{character}")?;
            }
        }
        Ok(())
    }
}

impl fmt::Display for LineMap<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "miri-lines {FORMAT_VERSION}")?;
        for (index, file) in self.files.iter().enumerate() {
            writeln!(f, "file {index} {}", FileName(file.name))?;
        }
        let lines: Vec<LineIndex> = self
            .files
            .iter()
            .map(|file| LineIndex::new(file.text))
            .collect();
        for (index, function) in self.functions.iter().enumerate() {
            writeln!(
                f,
                "fn {index} {} size={}",
                function.symbol, function.code_size
            )?;
            for run in line_runs(function, &lines) {
                write!(f, "  {} {} {}:{}", run.start, run.end, run.file, run.line)?;
                for (file, line) in &run.inlined_at {
                    write!(f, " <- {file}:{line}")?;
                }
                writeln!(f)?;
            }
        }
        Ok(())
    }
}

/// A run of a function's code generated from one source line.
#[derive(Debug, Clone, PartialEq, Eq)]
struct LineRun {
    start: u32,
    end: u32,
    file: SourceId,
    line: usize,
    /// The lines of the calls the run was inlined through, innermost first.
    inlined_at: Vec<(SourceId, usize)>,
}

/// The runs of `function`'s code, each on the line of its own file its
/// source offset falls on, adjacent runs on one line of one file inlined
/// through the same calls merged. An offset past the end of its file names
/// no line of it: a run at one is left out, and so is a call site at one.
fn line_runs(function: &FunctionCodeMap, lines: &[LineIndex]) -> Vec<LineRun> {
    let line_of = |source: SourceId, offset: u32| {
        lines
            .get(source.index())
            .and_then(|index| index.line_of(offset as usize))
    };
    let mut runs: Vec<LineRun> = Vec::with_capacity(function.ranges.len());
    for range in &function.ranges {
        let file = range.source;
        let Some(line) = line_of(file, range.source_offset) else {
            continue;
        };
        let inlined_at: Vec<(SourceId, usize)> = (range.inlined_at.iter())
            .filter_map(|call| Some((call.source, line_of(call.source, call.offset)?)))
            .collect();
        match runs.last_mut() {
            Some(last)
                if last.end == range.start
                    && last.file == file
                    && last.line == line
                    && last.inlined_at == inlined_at =>
            {
                last.end = range.end
            }
            _ => runs.push(LineRun {
                start: range.start,
                end: range.end,
                file,
                line,
                inlined_at,
            }),
        }
    }
    runs
}

/// The 1-based line of every byte offset into one text.
pub(crate) struct LineIndex {
    /// The offset each line starts at, the first line's 0 included.
    line_starts: Vec<usize>,
    /// The text's length, the last offset a line is found for.
    length: usize,
}

impl LineIndex {
    pub(crate) fn new(text: &str) -> Self {
        let line_starts = std::iter::once(0)
            .chain(
                text.bytes()
                    .enumerate()
                    .filter(|&(_, byte)| byte == b'\n')
                    .map(|(position, _)| position + 1),
            )
            .collect();
        Self {
            line_starts,
            length: text.len(),
        }
    }

    /// The line `offset` falls on: one more than the number of `\n` bytes
    /// before it. `None` past the end of the text.
    pub(crate) fn line_of(&self, offset: usize) -> Option<usize> {
        (offset <= self.length).then(|| {
            self.line_starts
                .partition_point(|&line_start| line_start <= offset)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codegen::{CodeRange, SourcePosition};

    #[test]
    fn the_first_line_is_line_one_and_a_newline_ends_its_line() {
        let index = LineIndex::new("ab\ncd\n");
        assert_eq!(index.line_of(0), Some(1));
        assert_eq!(index.line_of(2), Some(1));
        assert_eq!(index.line_of(3), Some(2));
        assert_eq!(index.line_of(6), Some(3));
    }

    #[test]
    fn a_carriage_return_stays_on_the_line_it_ends() {
        let index = LineIndex::new("ab\r\ncd\r\nef");
        assert_eq!(index.line_of(2), Some(1));
        assert_eq!(index.line_of(3), Some(1));
        assert_eq!(index.line_of(4), Some(2));
        assert_eq!(index.line_of(8), Some(3));
    }

    #[test]
    fn a_last_line_without_a_newline_reaches_the_end_of_the_text() {
        let index = LineIndex::new("one\ntwo");
        assert_eq!(index.line_of(4), Some(2));
        assert_eq!(index.line_of(7), Some(2));
        assert_eq!(index.line_of(8), None);
    }

    #[test]
    fn multibyte_characters_count_as_their_bytes() {
        let text = "é€\n🙂x\ny";
        let index = LineIndex::new(text);
        assert_eq!(text.find('x'), Some(10));
        assert_eq!(index.line_of(10), Some(2));
        assert_eq!(text.find('y'), Some(12));
        assert_eq!(index.line_of(12), Some(3));
    }

    #[test]
    fn an_empty_text_has_one_line() {
        let index = LineIndex::new("");
        assert_eq!(index.line_of(0), Some(1));
        assert_eq!(index.line_of(1), None);
    }

    fn range(start: u32, end: u32, source_offset: u32) -> CodeRange {
        CodeRange {
            start,
            end,
            source: SourceId::PROGRAM,
            source_offset,
            inlined_at: Vec::new(),
        }
    }

    #[test]
    fn adjacent_runs_on_one_line_merge_and_others_stay_apart() {
        let files = [
            SourceFile {
                name: "prog.mi",
                text: "fn f()\n    1\n",
            },
            SourceFile {
                name: "system/math.mi",
                text: "x\n",
            },
        ];
        let functions = [
            FunctionCodeMap {
                symbol: "miri.f".to_string(),
                code_size: 40,
                ranges: vec![
                    range(0, 4, 0),
                    range(4, 8, 3),
                    range(8, 12, 11),
                    range(16, 20, 11),
                    range(20, 24, 99),
                ],
            },
            FunctionCodeMap {
                symbol: "miri.$synthesized".to_string(),
                code_size: 8,
                ranges: Vec::new(),
            },
        ];
        let map = LineMap::new(&files, &functions).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            map.to_string(),
            "miri-lines 2\n\
             file 0 prog.mi\n\
             file 1 system/math.mi\n\
             fn 0 miri.f size=40\n  \
             0 8 0:1\n  \
             8 12 0:2\n  \
             16 20 0:2\n\
             fn 1 miri.$synthesized size=8\n"
        );
    }

    /// Code inlined from a function of another file is listed on that
    /// file's line, and a run of one file never merges into an adjacent run
    /// of another on a line of the same number.
    #[test]
    fn a_run_names_its_own_file() {
        let files = [
            SourceFile {
                name: "prog.mi",
                text: "fn f()\n    1\n",
            },
            SourceFile {
                name: "system/math.mi",
                text: "a\nb\n",
            },
        ];
        let library = SourceId::new(1);
        let inlined = |start, end, source_offset| CodeRange {
            source: library,
            ..range(start, end, source_offset)
        };
        let functions = [FunctionCodeMap {
            symbol: "miri.f".to_string(),
            code_size: 16,
            ranges: vec![
                range(0, 4, 0),
                inlined(4, 8, 0),
                inlined(8, 12, 2),
                range(12, 16, 11),
            ],
        }];
        let map = LineMap::new(&files, &functions).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            map.to_string(),
            "miri-lines 2\n\
             file 0 prog.mi\n\
             file 1 system/math.mi\n\
             fn 0 miri.f size=16\n  \
             0 4 0:1\n  \
             4 8 1:1\n  \
             8 12 1:2\n  \
             12 16 0:2\n"
        );
    }

    /// Code inlined through a chain of calls lists each call site after
    /// its own line, innermost first, and two runs on one line merge only
    /// when they were inlined through the same calls.
    #[test]
    fn a_run_lists_the_call_sites_it_was_inlined_through() {
        let files = [
            SourceFile {
                name: "prog.mi",
                text: "fn f()\n    1\n",
            },
            SourceFile {
                name: "system/math.mi",
                text: "a\nb\n",
            },
        ];
        let library = SourceId::new(1);
        let at = |source, offset| SourcePosition { source, offset };
        let inlined = |start, end, source_offset, inlined_at| CodeRange {
            source: library,
            inlined_at,
            ..range(start, end, source_offset)
        };
        let functions = [FunctionCodeMap {
            symbol: "miri.f".to_string(),
            code_size: 16,
            ranges: vec![
                inlined(0, 4, 0, vec![at(library, 2), at(SourceId::PROGRAM, 7)]),
                inlined(4, 8, 0, vec![at(library, 2), at(SourceId::PROGRAM, 8)]),
                inlined(8, 12, 0, vec![at(SourceId::PROGRAM, 0)]),
                inlined(12, 16, 0, vec![at(SourceId::PROGRAM, 0)]),
            ],
        }];
        let map = LineMap::new(&files, &functions).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            map.to_string(),
            "miri-lines 2\n\
             file 0 prog.mi\n\
             file 1 system/math.mi\n\
             fn 0 miri.f size=16\n  \
             0 8 1:1 <- 1:2 <- 0:2\n  \
             8 16 1:1 <- 0:1\n"
        );
    }

    fn function(symbol: &str) -> FunctionCodeMap {
        FunctionCodeMap {
            symbol: symbol.to_string(),
            code_size: 4,
            ranges: Vec::new(),
        }
    }

    #[test]
    fn a_control_character_in_a_file_name_cannot_start_a_line() {
        let files = [SourceFile {
            name: "a\nfile 9 b\\c.mi",
            text: "",
        }];
        let functions = [function("main")];
        let map = LineMap::new(&files, &functions).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(
            map.to_string(),
            "miri-lines 2\nfile 0 a\\nfile 9 b\\\\c.mi\nfn 0 main size=4\n"
        );
    }

    #[test]
    fn a_backend_that_recorded_no_code_map_writes_no_line_map() {
        let files = [SourceFile {
            name: "prog.mi",
            text: "",
        }];
        let refusal = LineMap::new(&files, &[]).err().map(|e| e.to_string());
        assert!(
            refusal
                .as_deref()
                .is_some_and(|message| message.contains("--emit lines")),
            "{refusal:?}"
        );
    }
}
