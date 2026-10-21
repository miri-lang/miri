// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! FFI functions behind an f-string format spec (`f"{x:>8.2f}"`).
//!
//! The compiler validates every spec before it emits these calls, so the
//! arguments are already in range; each function still clamps them, because a
//! runtime entry point must not trust the values it is handed.

use super::{miri_rt_string_new, new_string, MiriString};
use crate::guard;

/// The widest field and the most decimals a spec may ask for. The compiler
/// refuses larger ones; these bounds keep a call that slipped past it from
/// allocating without limit.
const MAX_WIDTH: usize = 1000;
const MAX_PRECISION: usize = 100;

/// Writes `value` with `precision` digits after the point.
#[no_mangle]
pub extern "C" fn miri_rt_format_fixed(value: f64, precision: i64) -> *mut MiriString {
    let precision = precision.clamp(0, MAX_PRECISION as i64) as usize;
    new_string(&format!("{value:.precision$}"))
}

/// Writes the integer `value` in `radix` (2, 8, 10 or 16), its sign before
/// the digits as Python writes it: `-255` in hex is `-ff`. `unsigned` reads
/// the bits as an unsigned magnitude; `upper` writes hex digits in capitals.
#[no_mangle]
pub extern "C" fn miri_rt_format_radix(
    value: i64,
    radix: i64,
    upper: i64,
    unsigned: i64,
) -> *mut MiriString {
    let (negative, magnitude) = if unsigned != 0 {
        (false, value as u64)
    } else {
        (value < 0, value.unsigned_abs())
    };
    let digits = match (radix, upper != 0) {
        (2, _) => format!("{magnitude:b}"),
        (8, _) => format!("{magnitude:o}"),
        (16, false) => format!("{magnitude:x}"),
        (16, true) => format!("{magnitude:X}"),
        _ => magnitude.to_string(),
    };
    let text = if negative {
        format!("-{digits}")
    } else {
        digits
    };
    new_string(&text)
}

/// Pads `text` to `width` characters with `fill`.
///
/// `align` is 0 for left, 1 for right and 2 for centre, the extra character
/// of an odd centring going to the right as Python places it. With `zero`
/// set the field is padded with zeros after a leading sign, as a number is.
/// Text already as wide as the field is returned as it is.
///
/// # Safety
/// `text` must be a valid `MiriString` pointer, or null.
#[no_mangle]
pub unsafe extern "C" fn miri_rt_format_pad(
    text: *const MiriString,
    width: i64,
    align: i64,
    fill: i64,
    zero: i64,
) -> *mut MiriString {
    guard::guard_check(text as *mut u8);
    if text.is_null() {
        return miri_rt_string_new();
    }
    let text = (*text).as_str();
    let width = width.clamp(0, MAX_WIDTH as i64) as usize;
    let fill = u32::try_from(fill)
        .ok()
        .and_then(char::from_u32)
        .unwrap_or(' ');
    new_string(&padded(text, width, align, fill, zero != 0))
}

fn padded(text: &str, width: usize, align: i64, fill: char, zero: bool) -> String {
    let length = text.chars().count();
    if length >= width {
        return text.to_string();
    }
    let gap = width - length;
    if zero {
        let (sign, digits) = match text.strip_prefix('-') {
            Some(rest) => ("-", rest),
            None => ("", text),
        };
        return format!("{sign}{}{digits}", "0".repeat(gap));
    }
    let run = |count: usize| fill.to_string().repeat(count);
    match align {
        0 => format!("{text}{}", run(gap)),
        2 => format!("{}{text}{}", run(gap / 2), run(gap - gap / 2)),
        _ => format!("{}{text}", run(gap)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_padding_places_the_text_by_alignment() {
        assert_eq!(padded("ab", 4, 0, ' ', false), "ab  ");
        assert_eq!(padded("ab", 4, 1, ' ', false), "  ab");
        assert_eq!(padded("ab", 5, 2, '*', false), "*ab**");
    }

    #[test]
    fn test_zero_padding_goes_after_the_sign() {
        assert_eq!(padded("-5", 4, 1, ' ', true), "-005");
        assert_eq!(padded("3.142", 8, 1, ' ', true), "0003.142");
    }

    #[test]
    fn test_text_wider_than_the_field_is_kept_whole() {
        assert_eq!(padded("abcdef", 3, 1, ' ', false), "abcdef");
    }

    #[test]
    fn test_width_counts_characters_not_bytes() {
        assert_eq!(padded("é", 3, 1, ' ', false), "  é");
    }
}
