// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Division and remainder for the 128-bit integer widths.
//!
//! The code generator has no instruction for these: its backend lowers the
//! narrower widths to a hardware divide, and at 128 bits there is nothing to
//! lower to — the operation is a library routine on every target this compiles
//! for. Rust reaches that same routine for a `i128 / i128`, so the whole job
//! here is to be callable from compiled code.
//!
//! Operands travel as their two 64-bit halves and the result is written through
//! a pointer, because a 128-bit value has no single register to arrive in and
//! passing one by value across this boundary would rest on the two sides
//! agreeing about a register pair.

/// Reassembles a 128-bit unsigned value from the halves it was passed as.
fn from_halves(lo: u64, hi: u64) -> u128 {
    ((hi as u128) << 64) | (lo as u128)
}

/// Writes a 128-bit value out through `out` as its two 64-bit halves.
///
/// # Safety
///
/// `out` must point to sixteen writable bytes.
unsafe fn write_halves(out: *mut u64, value: u128) {
    out.write_unaligned(value as u64);
    out.add(1).write_unaligned((value >> 64) as u64);
}

/// FFI surface for 128-bit division and remainder.
pub mod ffi {
    use super::{from_halves, write_halves};

    /// Divides two 128-bit signed integers, truncating toward zero.
    ///
    /// Overflow (the most negative value divided by `-1`) wraps to the most
    /// negative value, which is what the narrower widths do here too. A zero
    /// divisor is reported before this is reached, so it writes zero rather than
    /// bringing the process down on a path that cannot be taken.
    ///
    /// # Safety
    ///
    /// `out` must point to sixteen writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_i128_div(
        lhs_lo: u64,
        lhs_hi: u64,
        rhs_lo: u64,
        rhs_hi: u64,
        out: *mut u64,
    ) {
        if out.is_null() {
            return;
        }
        let rhs = from_halves(rhs_lo, rhs_hi) as i128;
        if rhs == 0 {
            write_halves(out, 0);
            return;
        }
        let lhs = from_halves(lhs_lo, lhs_hi) as i128;
        write_halves(out, lhs.wrapping_div(rhs) as u128);
    }

    /// The remainder of dividing two 128-bit signed integers, taking the sign of
    /// the dividend. Overflow wraps to zero, as it does at the narrower widths.
    ///
    /// # Safety
    ///
    /// `out` must point to sixteen writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_i128_rem(
        lhs_lo: u64,
        lhs_hi: u64,
        rhs_lo: u64,
        rhs_hi: u64,
        out: *mut u64,
    ) {
        if out.is_null() {
            return;
        }
        let rhs = from_halves(rhs_lo, rhs_hi) as i128;
        if rhs == 0 {
            write_halves(out, 0);
            return;
        }
        let lhs = from_halves(lhs_lo, lhs_hi) as i128;
        write_halves(out, lhs.wrapping_rem(rhs) as u128);
    }

    /// Divides two 128-bit unsigned integers.
    ///
    /// # Safety
    ///
    /// `out` must point to sixteen writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_u128_div(
        lhs_lo: u64,
        lhs_hi: u64,
        rhs_lo: u64,
        rhs_hi: u64,
        out: *mut u64,
    ) {
        if out.is_null() {
            return;
        }
        let rhs = from_halves(rhs_lo, rhs_hi);
        if rhs == 0 {
            write_halves(out, 0);
            return;
        }
        write_halves(out, from_halves(lhs_lo, lhs_hi) / rhs);
    }

    /// The remainder of dividing two 128-bit unsigned integers.
    ///
    /// # Safety
    ///
    /// `out` must point to sixteen writable bytes.
    #[no_mangle]
    pub unsafe extern "C" fn miri_rt_u128_rem(
        lhs_lo: u64,
        lhs_hi: u64,
        rhs_lo: u64,
        rhs_hi: u64,
        out: *mut u64,
    ) {
        if out.is_null() {
            return;
        }
        let rhs = from_halves(rhs_lo, rhs_hi);
        if rhs == 0 {
            write_halves(out, 0);
            return;
        }
        write_halves(out, from_halves(lhs_lo, lhs_hi) % rhs);
    }
}
