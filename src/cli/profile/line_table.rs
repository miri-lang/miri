// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading the line map a profile build writes beside its executable, to
//! find the source line a function's code offset was compiled from. The
//! format is the one `crate::pipeline::line_map` writes.

/// The version line a line map this reader understands starts with.
const VERSION_LINE: &str = "miri-lines 1";

/// A source line: the file's index in the map's file table, and the line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub(crate) struct SourceLine {
    pub file: usize,
    pub line: usize,
}

/// Where a run of a function's code compiled from one line starts. Its end
/// is read and checked but not kept: an offset past it is charged to it
/// all the same, as the nearest run before the offset.
#[derive(Debug, Clone, Copy)]
struct LineRun {
    start: u32,
    source: SourceLine,
}

/// One function of the map.
#[derive(Debug)]
struct MappedFunction {
    symbol: String,
    runs: Vec<LineRun>,
}

/// A build's line map.
#[derive(Debug, Default)]
pub(crate) struct LineTable {
    /// Every file, by its index, as the map names it.
    pub files: Vec<String>,
    functions: Vec<MappedFunction>,
}

impl LineTable {
    /// Read a line map, or say which line of it could not be read.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(VERSION_LINE) {
            return Err(format!("it does not start with `{VERSION_LINE}`"));
        }
        let mut table = Self::default();
        for (number, line) in (2..).zip(lines) {
            table
                .read_line(line)
                .ok_or_else(|| format!("line {number} is not a line-map line: `{line}`"))?;
        }
        Ok(table)
    }

    /// Take in one line after the version line.
    fn read_line(&mut self, line: &str) -> Option<()> {
        if let Some(entry) = line.strip_prefix("file ") {
            let (_, name) = entry.split_once(' ')?;
            self.files.push(unescape(name));
            return Some(());
        }
        if let Some(entry) = line.strip_prefix("fn ") {
            let mut words = entry.split(' ');
            let _index = words.next()?;
            let symbol = words.next()?.to_string();
            self.functions.push(MappedFunction {
                symbol,
                runs: Vec::new(),
            });
            return Some(());
        }
        let mut words = line.strip_prefix("  ")?.split(' ');
        let start = words.next()?.parse().ok()?;
        words.next()?.parse::<u32>().ok()?;
        let (file, line) = words.next()?.split_once(':')?;
        let source = SourceLine {
            file: file.parse().ok()?,
            line: line.parse().ok()?,
        };
        let function = self.functions.last_mut()?;
        function.runs.push(LineRun { start, source });
        Some(())
    }

    /// The line the code at `offset` into function `index` is charged to,
    /// as a debugger's line table charges it: the run holding the offset, or
    /// else the nearest run starting before it, since an instruction the
    /// backend gave no source location (a branch, an overflow check) belongs
    /// to the statement before it; before the first run, in the prologue,
    /// the first run. `None` only for a function the map lists no run for.
    pub(crate) fn line_at(&self, index: usize, offset: u32) -> Option<SourceLine> {
        let runs = &self.functions.get(index)?.runs;
        let after = runs.partition_point(|run| run.start <= offset);
        let run = after
            .checked_sub(1)
            .and_then(|before| runs.get(before))
            .or_else(|| runs.first())?;
        Some(run.source)
    }

    /// The link symbol of function `index`.
    pub(crate) fn symbol(&self, index: usize) -> Option<&str> {
        self.functions
            .get(index)
            .map(|function| function.symbol.as_str())
    }
}

/// A file name as written, its escapes undone: `\\`, `\n`, `\r`, `\t` and
/// `\u{...}`, the escapes the writer uses.
fn unescape(name: &str) -> String {
    let mut text = String::with_capacity(name.len());
    let mut characters = name.chars();
    while let Some(character) = characters.next() {
        if character != '\\' {
            text.push(character);
            continue;
        }
        match characters.next() {
            Some('n') => text.push('\n'),
            Some('r') => text.push('\r'),
            Some('t') => text.push('\t'),
            Some('u') => {
                let code: String = characters
                    .by_ref()
                    .skip_while(|&c| c == '{')
                    .take_while(|&c| c != '}')
                    .collect();
                let decoded = u32::from_str_radix(&code, 16).ok().and_then(char::from_u32);
                text.extend(decoded);
            }
            Some(other) => text.push(other),
            None => text.push('\\'),
        }
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAP: &str = "miri-lines 1\nfile 0 prog.mi\nfile 1 system/math.mi\n\
                       fn 0 miri.spin size=64\n  0 8 0:3\n  8 40 0:8\n  48 60 1:12\n\
                       fn 1 main size=20\n\
                       fn 2 miri.later size=40\n  16 24 0:5\n  24 32 0:6\n";

    #[test]
    fn an_offset_is_found_on_the_line_its_run_was_compiled_from() {
        let table = LineTable::parse(MAP).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(table.files, vec!["prog.mi", "system/math.mi"]);
        assert_eq!(table.line_at(0, 0), Some(SourceLine { file: 0, line: 3 }));
        assert_eq!(table.line_at(0, 39), Some(SourceLine { file: 0, line: 8 }));
        assert_eq!(table.line_at(0, 50), Some(SourceLine { file: 1, line: 12 }));
        assert_eq!(table.symbol(1), Some("main"));
    }

    #[test]
    fn an_offset_outside_every_run_takes_the_nearest_run_before_it() {
        let table = LineTable::parse(MAP).unwrap_or_else(|e| panic!("{e}"));
        // In the gap between two runs: the run before it.
        assert_eq!(table.line_at(0, 44), Some(SourceLine { file: 0, line: 8 }));
        // Past the last run: the last run.
        assert_eq!(table.line_at(0, 60), Some(SourceLine { file: 1, line: 12 }));
        assert_eq!(table.line_at(2, 36), Some(SourceLine { file: 0, line: 6 }));
        // Before the first run, in the prologue: the first run.
        assert_eq!(table.line_at(2, 4), Some(SourceLine { file: 0, line: 5 }));
    }

    #[test]
    fn a_function_with_no_runs_or_none_at_all_has_no_line() {
        let table = LineTable::parse(MAP).unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(table.line_at(1, 4), None);
        assert_eq!(table.line_at(7, 0), None);
    }

    #[test]
    fn an_escaped_file_name_is_read_back() {
        let table = LineTable::parse("miri-lines 1\nfile 0 a\\nb\\\\c\\u{7f}.mi\n")
            .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(table.files, vec!["a\nb\\c\u{7f}.mi"]);
    }

    #[test]
    fn a_line_map_in_another_format_is_refused_with_the_line() {
        assert!(LineTable::parse("miri-lines 2\n").is_err());
        let refusal = LineTable::parse("miri-lines 1\n  0 4 0:1\n").err();
        assert_eq!(
            refusal.as_deref(),
            Some("line 2 is not a line-map line: `  0 4 0:1`")
        );
    }
}
