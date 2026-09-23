// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! Host/device element conversion for GPU buffers and captured scalars.
//!
//! The compiler's GPU wire format decides, for every element type, the width
//! it has on the device and how the host converts it: most integer widths
//! travel in a 32-bit lane, so the host widens `i8`/`u8`/`i16`/`u16` elements
//! on upload and truncates them back on readback, range-checks `i64`/`u64`
//! elements into the lane on upload and extends them on readback, and rounds a
//! host `f64` (`float`) into an `f32` lane. The launch descriptor carries one
//! conversion code per buffer; the numbering below is that ABI and must match
//! the compiler's `WireConversion` discriminants.

use crate::context::GpuError;
use std::io::Write;

/// Width of every converted element on the device: all conversions target a
/// 32-bit lane.
const LANE_BYTES: usize = 4;

/// How one buffer's elements are converted between host and device bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireConversion {
    Identity,
    NarrowI64,
    NarrowU64,
    WidenI8,
    WidenU8,
    WidenI16,
    WidenU16,
    DemoteF64,
}

impl WireConversion {
    /// Decodes a descriptor conversion code; `None` for a code the compiler
    /// never emits.
    pub fn from_code(code: u8) -> Option<Self> {
        let conversion = match code {
            0 => WireConversion::Identity,
            1 => WireConversion::NarrowI64,
            2 => WireConversion::NarrowU64,
            3 => WireConversion::WidenI8,
            4 => WireConversion::WidenU8,
            5 => WireConversion::WidenI16,
            6 => WireConversion::WidenU16,
            7 => WireConversion::DemoteF64,
            _ => return None,
        };
        Some(conversion)
    }

    /// The conversion for buffer `index` of a descriptor's code array, which
    /// is absent (null) when no buffer needs converting.
    pub fn for_buffer(codes: Option<&[u8]>, index: usize) -> Result<Self, GpuError> {
        let Some(&code) = codes.and_then(|codes| codes.get(index)) else {
            return Ok(WireConversion::Identity);
        };
        WireConversion::from_code(code).ok_or_else(|| {
            GpuError::UnsupportedScalar(format!(
                "buffer {index} carries unknown element conversion code {code}"
            ))
        })
    }

    /// Host bytes per element, or `None` for `Identity`, which copies bytes
    /// without regard to element boundaries.
    fn host_element_bytes(self) -> Option<usize> {
        match self {
            WireConversion::Identity => None,
            WireConversion::WidenI8 | WireConversion::WidenU8 => Some(1),
            WireConversion::WidenI16 | WireConversion::WidenU16 => Some(2),
            WireConversion::NarrowI64 | WireConversion::NarrowU64 | WireConversion::DemoteF64 => {
                Some(8)
            }
        }
    }

    pub fn is_identity(self) -> bool {
        self == WireConversion::Identity
    }

    /// Device byte length of a buffer whose host copy is `host_len` bytes.
    pub fn device_len(self, host_len: usize) -> Result<usize, GpuError> {
        let Some(element_bytes) = self.host_element_bytes() else {
            return Ok(host_len);
        };
        require_whole_elements(host_len, element_bytes)?;
        (host_len / element_bytes)
            .checked_mul(LANE_BYTES)
            .ok_or_else(|| GpuError::GridTooLarge("device buffer size overflows".to_string()))
    }

    /// The longest prefix of a `host_len`-byte host buffer, in whole
    /// elements, whose device image fits in `device_len` bytes — so a readback
    /// never copies more than the device buffer holds, whatever the host
    /// length has drifted to.
    pub fn host_len_within(self, host_len: usize, device_len: usize) -> usize {
        let Some(element_bytes) = self.host_element_bytes() else {
            return host_len.min(device_len);
        };
        let elements = (host_len / element_bytes).min(device_len / LANE_BYTES);
        elements * element_bytes
    }

    /// Converts a host buffer to its device bytes, refusing any element a
    /// narrowing lane cannot hold. `buffer_index` names the buffer in the
    /// error.
    pub fn encode(self, host: &[u8], buffer_index: usize) -> Result<Vec<u8>, GpuError> {
        match self {
            WireConversion::Identity => Ok(host.to_vec()),
            WireConversion::NarrowI64 => encode_lanes(host, |bytes, element_index| {
                let value = i64::from_le_bytes(bytes);
                i32::try_from(value).map(i32::to_le_bytes).map_err(|_| {
                    GpuError::ValueOutOfI32Range {
                        buffer_index,
                        element_index,
                        value,
                    }
                })
            }),
            WireConversion::NarrowU64 => encode_lanes(host, |bytes, element_index| {
                let value = u64::from_le_bytes(bytes);
                u32::try_from(value).map(u32::to_le_bytes).map_err(|_| {
                    GpuError::ValueOutOfU32Range {
                        buffer_index,
                        element_index,
                        value,
                    }
                })
            }),
            WireConversion::WidenI8 => encode_lanes(host, |b: [u8; 1], _| {
                Ok(i32::from(i8::from_le_bytes(b)).to_le_bytes())
            }),
            WireConversion::WidenU8 => {
                encode_lanes(host, |b: [u8; 1], _| Ok(u32::from(b[0]).to_le_bytes()))
            }
            WireConversion::WidenI16 => encode_lanes(host, |b: [u8; 2], _| {
                Ok(i32::from(i16::from_le_bytes(b)).to_le_bytes())
            }),
            WireConversion::WidenU16 => encode_lanes(host, |b: [u8; 2], _| {
                Ok(u32::from(u16::from_le_bytes(b)).to_le_bytes())
            }),
            WireConversion::DemoteF64 => encode_lanes(host, |b: [u8; 8], _| {
                Ok((f64::from_le_bytes(b) as f32).to_le_bytes())
            }),
        }
    }

    /// Writes device bytes back into the host buffer, converting each lane to
    /// the host element width. A sub-word element keeps the low bytes of its
    /// lane, exactly as storing the lane's value into the element would.
    ///
    /// A converted buffer must be whole elements on both sides with one lane
    /// per host element; anything else is refused rather than leaving part of
    /// the host buffer stale.
    pub fn decode(self, device: &[u8], host: &mut [u8]) -> Result<(), GpuError> {
        if self.is_identity() {
            let len = host.len().min(device.len());
            host[..len].copy_from_slice(&device[..len]);
            return Ok(());
        }
        if self.device_len(host.len())? != device.len() {
            return Err(GpuError::UnsupportedScalar(format!(
                "device buffer of {} bytes does not hold one {LANE_BYTES}-byte lane per element of a {}-byte host buffer",
                device.len(),
                host.len()
            )));
        }
        match self {
            WireConversion::Identity => {}
            WireConversion::NarrowI64 => decode_lanes(device, host, |lane| {
                i64::from(i32::from_le_bytes(lane)).to_le_bytes()
            }),
            WireConversion::NarrowU64 => decode_lanes(device, host, |lane| {
                u64::from(u32::from_le_bytes(lane)).to_le_bytes()
            }),
            WireConversion::WidenI8 | WireConversion::WidenU8 => {
                decode_lanes(device, host, |lane| [lane[0]])
            }
            WireConversion::WidenI16 | WireConversion::WidenU16 => {
                decode_lanes(device, host, |lane| [lane[0], lane[1]])
            }
            WireConversion::DemoteF64 => decode_lanes(device, host, |lane| {
                f64::from(f32::from_le_bytes(lane)).to_le_bytes()
            }),
        }
        Ok(())
    }
}

/// Refuses a byte length that is not a whole number of `element_bytes`-byte
/// elements, so no conversion silently drops a trailing partial element.
fn require_whole_elements(len: usize, element_bytes: usize) -> Result<(), GpuError> {
    if len.is_multiple_of(element_bytes) {
        return Ok(());
    }
    Err(GpuError::UnsupportedScalar(format!(
        "host buffer of {len} bytes is not a whole number of {element_bytes}-byte elements"
    )))
}

/// Converts each `N`-byte host element into one device lane.
fn encode_lanes<const N: usize>(
    host: &[u8],
    convert: impl Fn([u8; N], usize) -> Result<[u8; LANE_BYTES], GpuError>,
) -> Result<Vec<u8>, GpuError> {
    require_whole_elements(host.len(), N)?;
    let mut device = Vec::with_capacity(host.len() / N * LANE_BYTES);
    for (element_index, bytes) in host.chunks_exact(N).enumerate() {
        device.extend_from_slice(&convert(to_array(bytes), element_index)?);
    }
    Ok(device)
}

/// Converts each device lane back into one `N`-byte host element.
fn decode_lanes<const N: usize>(
    device: &[u8],
    host: &mut [u8],
    convert: impl Fn([u8; LANE_BYTES]) -> [u8; N],
) {
    for (element, lane) in host
        .chunks_exact_mut(N)
        .zip(device.chunks_exact(LANE_BYTES))
    {
        element.copy_from_slice(&convert(to_array(lane)));
    }
}

/// Copies an exactly-`N`-byte chunk into an array; callers pass chunks from
/// `chunks_exact(N)`, so the lengths always agree.
fn to_array<const N: usize>(chunk: &[u8]) -> [u8; N] {
    let mut array = [0u8; N];
    array.copy_from_slice(chunk);
    array
}

/// The report for a captured scalar whose run-time value does not fit the
/// 32-bit lane it travels in. `value` carries the scalar's 64 bits; the
/// conversion decides whether they read as signed or unsigned.
pub fn capture_out_of_range_message(scalar_index: u64, value: i64, conversion_code: u8) -> String {
    let (value, lane, min, max) = match WireConversion::from_code(conversion_code) {
        Some(WireConversion::NarrowU64) => (
            (value as u64).to_string(),
            "u32",
            i64::from(u32::MIN),
            i64::from(u32::MAX),
        ),
        Some(
            WireConversion::Identity
            | WireConversion::NarrowI64
            | WireConversion::WidenI8
            | WireConversion::WidenU8
            | WireConversion::WidenI16
            | WireConversion::WidenU16
            | WireConversion::DemoteF64,
        )
        | None => (
            value.to_string(),
            "i32",
            i64::from(i32::MIN),
            i64::from(i32::MAX),
        ),
    };
    format!(
        "Runtime error: GPU launch failed: captured scalar {scalar_index} has value {value} \
         which exceeds {lane} range [{min}, {max}], the 32-bit lane it travels in on the device"
    )
}

/// Reports a captured scalar that does not fit its device lane. The compiler
/// range-checks every narrowed capture before the launch and calls this only
/// on a value it cannot pass without corrupting it; the generated code then
/// leaves through the core runtime's launch-failure exit, so the program ends
/// with a registered runtime code instead of a signal.
#[no_mangle]
pub extern "C" fn miri_gpu_capture_out_of_range(
    scalar_index: u64,
    value: i64,
    conversion_code: u8,
) {
    let message = capture_out_of_range_message(scalar_index, value, conversion_code);
    let _ = writeln!(std::io::stderr(), "{}", message);
}
