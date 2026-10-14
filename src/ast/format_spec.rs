// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The format spec an f-string interpolation may carry after `:`.
//!
//! Miri reads Python's spelling, `[[fill]align][0][width][.precision][type]`,
//! restricted to the parts every target can honour the same way: alignment
//! with a fill character, zero padding, a width, a precision for fixed-point
//! floats, and a radix for integers.

use crate::error::syntax::Span;

/// A format spec as written: the text after the `:`, and where it sits.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct FormatSpec {
    pub text: String,
    pub span: Span,
}

/// Where a value sits in a field wider than it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
    Center,
}

/// How a number is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Presentation {
    /// `f`: a float, or an integer written as one, with a fixed count of
    /// decimals.
    Fixed,
    /// `d`: an integer in decimal.
    Decimal,
    /// `x`: an integer in lowercase hexadecimal.
    Hex,
    /// `X`: an integer in uppercase hexadecimal.
    UpperHex,
    /// `b`: an integer in binary.
    Binary,
    /// `o`: an integer in octal.
    Octal,
}

impl Presentation {
    /// The radix an integer presentation writes in, or `None` for `f`.
    pub fn radix(self) -> Option<u32> {
        match self {
            Self::Fixed => None,
            Self::Decimal => Some(10),
            Self::Hex | Self::UpperHex => Some(16),
            Self::Binary => Some(2),
            Self::Octal => Some(8),
        }
    }
}

/// A format spec read into its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedSpec {
    pub fill: char,
    pub align: Option<Align>,
    pub zero: bool,
    pub width: Option<u32>,
    pub precision: Option<u32>,
    pub presentation: Option<Presentation>,
}

/// Why a spec cannot be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpecError {
    /// The text is not `[[fill]align][0][width][.precision][type]`.
    Unreadable,
    /// A precision with no type: Python reads `.2` as significant digits and
    /// Rust as decimals, so Miri asks for `f`.
    PrecisionWithoutType(u32),
    /// A width or precision past [`MAX_WIDTH`] or [`MAX_PRECISION`].
    TooLarge,
}

/// The widest field a spec may ask for.
pub const MAX_WIDTH: u32 = 1000;

/// The most decimals a spec may ask for.
pub const MAX_PRECISION: u32 = 100;

/// The grammar every spec is written in, for messages.
pub const SPEC_SHAPE: &str = "[[fill]align][0][width][.precision][type]";

/// Reads `text`, the characters after an interpolation's `:`.
pub fn parse(text: &str) -> Result<ParsedSpec, SpecError> {
    let chars: Vec<char> = text.chars().collect();
    let mut spec = ParsedSpec {
        fill: ' ',
        align: None,
        zero: false,
        width: None,
        precision: None,
        presentation: None,
    };
    let mut at = 0;
    if let Some(align) = chars.get(1).copied().and_then(align_of) {
        spec.fill = chars[0];
        spec.align = Some(align);
        at = 2;
    } else if let Some(align) = chars.first().copied().and_then(align_of) {
        spec.align = Some(align);
        at = 1;
    }
    if chars.get(at) == Some(&'0') {
        spec.zero = true;
        at += 1;
    }
    spec.width = digits(&chars, &mut at)?;
    if chars.get(at) == Some(&'.') {
        at += 1;
        spec.precision = Some(digits(&chars, &mut at)?.ok_or(SpecError::Unreadable)?);
    }
    if let Some(presentation) = chars.get(at).copied().and_then(presentation_of) {
        spec.presentation = Some(presentation);
        at += 1;
    }
    if at != chars.len() {
        return Err(SpecError::Unreadable);
    }
    check_limits(&spec)?;
    Ok(spec)
}

/// A precision belongs to `f` alone, and both numbers stay within bounds.
fn check_limits(spec: &ParsedSpec) -> Result<(), SpecError> {
    match (spec.precision, spec.presentation) {
        (Some(precision), None) => return Err(SpecError::PrecisionWithoutType(precision)),
        (Some(_), Some(presentation)) if presentation != Presentation::Fixed => {
            return Err(SpecError::Unreadable)
        }
        _ => {}
    }
    let too_wide = spec.width.is_some_and(|width| width > MAX_WIDTH);
    let too_precise = spec
        .precision
        .is_some_and(|precision| precision > MAX_PRECISION);
    if too_wide || too_precise {
        return Err(SpecError::TooLarge);
    }
    Ok(())
}

/// The decimal number starting at `at`, advancing past it; `None` when no
/// digit is there.
fn digits(chars: &[char], at: &mut usize) -> Result<Option<u32>, SpecError> {
    let start = *at;
    while chars.get(*at).is_some_and(|c| c.is_ascii_digit()) {
        *at += 1;
    }
    if start == *at {
        return Ok(None);
    }
    let text: String = chars[start..*at].iter().collect();
    text.parse().map(Some).map_err(|_| SpecError::TooLarge)
}

fn align_of(c: char) -> Option<Align> {
    match c {
        '<' => Some(Align::Left),
        '>' => Some(Align::Right),
        '^' => Some(Align::Center),
        _ => None,
    }
}

fn presentation_of(c: char) -> Option<Presentation> {
    match c {
        'f' => Some(Presentation::Fixed),
        'd' => Some(Presentation::Decimal),
        'x' => Some(Presentation::Hex),
        'X' => Some(Presentation::UpperHex),
        'b' => Some(Presentation::Binary),
        'o' => Some(Presentation::Octal),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_precision_with_f_reads() {
        let spec = parse(".2f").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(spec.precision, Some(2));
        assert_eq!(spec.presentation, Some(Presentation::Fixed));
    }

    #[test]
    fn test_fill_align_zero_width_and_type_read_together() {
        let spec = parse("*>08x").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!((spec.fill, spec.align), ('*', Some(Align::Right)));
        assert!(spec.zero);
        assert_eq!(spec.width, Some(8));
        assert_eq!(spec.presentation, Some(Presentation::Hex));
    }

    #[test]
    fn test_an_align_alone_has_the_default_fill() {
        let spec = parse("^6").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(
            (spec.fill, spec.align, spec.width),
            (' ', Some(Align::Center), Some(6))
        );
    }

    #[test]
    fn test_a_precision_without_a_type_is_refused() {
        assert_eq!(parse(".2"), Err(SpecError::PrecisionWithoutType(2)));
    }

    #[test]
    fn test_a_precision_on_a_radix_is_refused() {
        assert_eq!(parse(".2x"), Err(SpecError::Unreadable));
    }

    #[test]
    fn test_unknown_characters_are_refused() {
        assert_eq!(parse(","), Err(SpecError::Unreadable));
        assert_eq!(parse("+d"), Err(SpecError::Unreadable));
        assert_eq!(parse(".f"), Err(SpecError::Unreadable));
    }

    #[test]
    fn test_a_huge_width_is_refused() {
        assert_eq!(parse("1001"), Err(SpecError::TooLarge));
        assert_eq!(parse("99999999999"), Err(SpecError::TooLarge));
        assert_eq!(parse(".101f"), Err(SpecError::TooLarge));
    }

    #[test]
    fn test_an_empty_spec_asks_for_nothing() {
        let spec = parse("").unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(spec.width, None);
        assert_eq!(spec.presentation, None);
    }
}
