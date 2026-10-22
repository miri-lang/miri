// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Reading the record a profiled run writes to `MIRI_PROFILE_OUT`.
//!
//! The runtime's profiler documents the format: a version line, `hz`,
//! `samples`, `dropped` and `truncated`, then one `<count> <frame>...` line per distinct
//! stack, the interrupted instruction first. A frame is `f<index>+<offset>`
//! inside a function of the build's line map, or an address, optionally
//! followed by `:<symbol>+<offset>`.

/// The version line a record this reader understands starts with.
const VERSION_LINE: &str = "miri-profile 1";

/// One frame of a recorded stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RecordedFrame {
    /// A byte offset into the function the line map lists at `index`.
    Function { index: usize, offset: u32 },
    /// An address outside the line map's functions, with the symbol the
    /// loader named for it, if any.
    Other { symbol: Option<String> },
}

/// A stack and how many samples saw it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RecordedStack {
    pub count: u64,
    pub frames: Vec<RecordedFrame>,
}

/// A profile record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ProfileRecord {
    pub hz: u32,
    pub dropped: u64,
    /// The samples kept whose stacks the record does not list.
    pub truncated: u64,
    pub stacks: Vec<RecordedStack>,
}

impl ProfileRecord {
    /// The samples of the listed stacks, saturating: a record is read from
    /// a file, and its counts are not trusted to sum.
    pub(crate) fn samples(&self) -> u64 {
        self.stacks
            .iter()
            .fold(0u64, |sum, stack| sum.saturating_add(stack.count))
    }

    /// Read a record, or say which line of it could not be read.
    pub(crate) fn parse(text: &str) -> Result<Self, String> {
        let mut lines = text.lines();
        if lines.next() != Some(VERSION_LINE) {
            return Err(format!("it does not start with `{VERSION_LINE}`"));
        }
        let mut record = Self {
            hz: 0,
            dropped: 0,
            truncated: 0,
            stacks: Vec::new(),
        };
        for (number, line) in (2..).zip(lines) {
            record
                .read_line(line)
                .ok_or_else(|| format!("line {number} is not a record line: `{line}`"))?;
        }
        Ok(record)
    }

    /// Take in one line after the version line.
    fn read_line(&mut self, line: &str) -> Option<()> {
        let mut words = line.split(' ');
        let first = words.next()?;
        match first {
            "hz" => self.hz = words.next()?.parse().ok()?,
            "dropped" => self.dropped = words.next()?.parse().ok()?,
            "truncated" => self.truncated = words.next()?.parse().ok()?,
            "samples" => {
                words.next()?.parse::<u64>().ok()?;
            }
            count => {
                let count = count.parse().ok()?;
                let frames = words.map(frame).collect::<Option<Vec<_>>>()?;
                self.stacks.push(RecordedStack { count, frames });
            }
        }
        Some(())
    }
}

/// The frame `word` writes.
fn frame(word: &str) -> Option<RecordedFrame> {
    if let Some(address) = word.strip_prefix("0x") {
        let (digits, symbol) = match address.split_once(':') {
            Some((digits, named)) => (digits, Some(symbol_name(named))),
            None => (address, None),
        };
        usize::from_str_radix(digits, 16).ok()?;
        return Some(RecordedFrame::Other { symbol });
    }
    let (index, offset) = word.strip_prefix('f')?.split_once('+')?;
    Some(RecordedFrame::Function {
        index: index.parse().ok()?,
        offset: offset.parse().ok()?,
    })
}

/// The symbol of a `<symbol>+<offset>` name, its offset dropped.
fn symbol_name(named: &str) -> String {
    named
        .rsplit_once('+')
        .map_or(named, |(symbol, _)| symbol)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_is_read_into_its_stacks() {
        let record = ProfileRecord::parse(
            "miri-profile 1\nhz 500\nsamples 7\ndropped 2\n\
             3 f2+40 f0+8\n2 0x10a0:miri_rt_list_push+16 f2+12\n",
        )
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(record.hz, 500);
        assert_eq!(record.dropped, 2);
        assert_eq!(record.samples(), 5);
        assert_eq!(
            record.stacks[1].frames,
            vec![
                RecordedFrame::Other {
                    symbol: Some("miri_rt_list_push".to_string())
                },
                RecordedFrame::Function {
                    index: 2,
                    offset: 12
                },
            ]
        );
    }

    #[test]
    fn an_address_the_loader_named_nothing_for_has_no_symbol() {
        assert_eq!(
            frame("0xdeadbeef"),
            Some(RecordedFrame::Other { symbol: None })
        );
        assert_eq!(frame("0xnothex"), None);
    }

    #[test]
    fn a_record_in_another_format_is_refused_with_the_line() {
        assert!(ProfileRecord::parse("miri-profile 2\n").is_err());
        let refusal = ProfileRecord::parse("miri-profile 1\nhz 1000\n4 g1+2\n").err();
        assert_eq!(
            refusal.as_deref(),
            Some("line 3 is not a record line: `4 g1+2`")
        );
    }

    #[test]
    fn counts_past_any_real_run_saturate_rather_than_overflow() {
        let record = ProfileRecord::parse(&format!(
            "miri-profile 1\nhz 1\ndropped 3\ntruncated {max}\n{max} 0x10\n{max} 0x20\n",
            max = u64::MAX
        ))
        .unwrap_or_else(|e| panic!("{e}"));
        assert_eq!(record.truncated, u64::MAX);
        assert_eq!(record.samples(), u64::MAX);
    }
}
