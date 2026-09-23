// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

use miri_runtime_gpu::context::GpuError;
use miri_runtime_gpu::wire::{capture_out_of_range_message, WireConversion};

fn bytes_of<const N: usize>(elements: &[[u8; N]]) -> Vec<u8> {
    elements.concat()
}

fn round_trip(conversion: WireConversion, host: &[u8]) -> Vec<u8> {
    let device = conversion.encode(host, 0).expect("in-range upload");
    assert_eq!(
        device.len(),
        conversion.device_len(host.len()).expect("whole elements")
    );
    let mut back = vec![0u8; host.len()];
    conversion.decode(&device, &mut back).expect("whole lanes");
    back
}

#[test]
fn codes_decode_to_the_compiler_numbering() {
    let expected = [
        WireConversion::Identity,
        WireConversion::NarrowI64,
        WireConversion::NarrowU64,
        WireConversion::WidenI8,
        WireConversion::WidenU8,
        WireConversion::WidenI16,
        WireConversion::WidenU16,
        WireConversion::DemoteF64,
    ];
    for (code, conversion) in expected.into_iter().enumerate() {
        assert_eq!(WireConversion::from_code(code as u8), Some(conversion));
    }
    assert_eq!(WireConversion::from_code(8), None);
}

#[test]
fn unknown_code_for_a_buffer_is_an_error_and_absent_codes_are_identity() {
    assert_eq!(
        WireConversion::for_buffer(None, 3).expect("absent codes"),
        WireConversion::Identity
    );
    assert!(matches!(
        WireConversion::for_buffer(Some(&[9]), 0),
        Err(GpuError::UnsupportedScalar(_))
    ));
}

#[test]
fn every_converted_element_occupies_one_32_bit_lane() {
    assert_eq!(WireConversion::WidenI8.device_len(3).expect("i8"), 12);
    assert_eq!(WireConversion::WidenU16.device_len(6).expect("u16"), 12);
    assert_eq!(WireConversion::NarrowI64.device_len(16).expect("i64"), 8);
    assert_eq!(WireConversion::DemoteF64.device_len(24).expect("f64"), 12);
    assert_eq!(WireConversion::Identity.device_len(10).expect("raw"), 10);
}

#[test]
fn partial_element_is_refused_rather_than_truncated() {
    assert!(matches!(
        WireConversion::NarrowI64.device_len(12),
        Err(GpuError::UnsupportedScalar(_))
    ));
}

#[test]
fn sub_word_signed_elements_sign_extend_into_their_lane() {
    let device = WireConversion::WidenI16
        .encode(&bytes_of(&[(-300i16).to_le_bytes(), 7i16.to_le_bytes()]), 0)
        .expect("upload");
    assert_eq!(
        device,
        bytes_of(&[(-300i32).to_le_bytes(), 7i32.to_le_bytes()])
    );
    let device = WireConversion::WidenI8
        .encode(&[(-5i8) as u8], 0)
        .expect("upload");
    assert_eq!(device, (-5i32).to_le_bytes());
}

#[test]
fn sub_word_unsigned_elements_zero_extend_into_their_lane() {
    let device = WireConversion::WidenU8.encode(&[200], 0).expect("upload");
    assert_eq!(device, 200u32.to_le_bytes());
    let device = WireConversion::WidenU16
        .encode(&60000u16.to_le_bytes(), 0)
        .expect("upload");
    assert_eq!(device, 60000u32.to_le_bytes());
}

#[test]
fn every_conversion_round_trips_an_in_range_host_buffer() {
    let cases: [(WireConversion, Vec<u8>); 7] = [
        (WireConversion::WidenI8, vec![0x80, 0x7f, 0xff]),
        (WireConversion::WidenU8, vec![0, 200, 255]),
        (
            WireConversion::WidenI16,
            bytes_of(&[i16::MIN.to_le_bytes(), i16::MAX.to_le_bytes()]),
        ),
        (
            WireConversion::WidenU16,
            bytes_of(&[0u16.to_le_bytes(), u16::MAX.to_le_bytes()]),
        ),
        (
            WireConversion::NarrowI64,
            bytes_of(&[i64::from(i32::MIN).to_le_bytes(), 9i64.to_le_bytes()]),
        ),
        (
            WireConversion::NarrowU64,
            bytes_of(&[u64::from(u32::MAX).to_le_bytes(), 9u64.to_le_bytes()]),
        ),
        (WireConversion::DemoteF64, bytes_of(&[2.5f64.to_le_bytes()])),
    ];
    for (conversion, host) in cases {
        assert_eq!(round_trip(conversion, &host), host, "{conversion:?}");
    }
}

#[test]
fn readback_truncates_a_lane_to_the_sub_word_element() {
    let mut host = [0u8; 1];
    WireConversion::WidenU8
        .decode(&300u32.to_le_bytes(), &mut host)
        .expect("one lane");
    assert_eq!(host, [44]);
}

#[test]
fn upload_of_a_partial_trailing_element_is_refused() {
    assert!(matches!(
        WireConversion::WidenI16.encode(&[1, 2, 3], 0),
        Err(GpuError::UnsupportedScalar(_))
    ));
    assert!(matches!(
        WireConversion::NarrowI64.encode(&[0; 12], 0),
        Err(GpuError::UnsupportedScalar(_))
    ));
}

#[test]
fn readback_into_a_partial_trailing_element_is_refused() {
    let mut host = [0u8; 12];
    assert!(matches!(
        WireConversion::NarrowI64.decode(&[0; 8], &mut host),
        Err(GpuError::UnsupportedScalar(_))
    ));
}

#[test]
fn readback_with_a_lane_count_that_disagrees_with_the_host_is_refused() {
    let mut host = [0u8; 2];
    assert!(matches!(
        WireConversion::WidenU8.decode(&[0; 4], &mut host),
        Err(GpuError::UnsupportedScalar(_))
    ));
    let mut host = [0u8; 8];
    assert!(matches!(
        WireConversion::DemoteF64.decode(&[0; 3], &mut host),
        Err(GpuError::UnsupportedScalar(_))
    ));
}

#[test]
fn readback_length_is_clamped_to_the_elements_the_device_holds() {
    assert_eq!(WireConversion::Identity.host_len_within(40, 16), 16);
    assert_eq!(WireConversion::Identity.host_len_within(8, 16), 8);
    assert_eq!(WireConversion::NarrowI64.host_len_within(32, 8), 16);
    assert_eq!(WireConversion::NarrowI64.host_len_within(16, 64), 16);
    assert_eq!(WireConversion::WidenU8.host_len_within(10, 16), 4);
    assert_eq!(WireConversion::WidenI16.host_len_within(6, 6), 2);
    assert_eq!(WireConversion::DemoteF64.host_len_within(24, 0), 0);
}

#[test]
fn i64_element_outside_the_i32_lane_is_refused_with_its_position() {
    let host = bytes_of(&[1i64.to_le_bytes(), 5_000_000_000i64.to_le_bytes()]);
    match WireConversion::NarrowI64.encode(&host, 2) {
        Err(GpuError::ValueOutOfI32Range {
            buffer_index: 2,
            element_index: 1,
            value: 5_000_000_000,
        }) => {}
        other => panic!("expected an i32 range error, got {other:?}"),
    }
}

#[test]
fn u64_element_outside_the_u32_lane_is_refused_with_its_position() {
    let host = (u64::from(u32::MAX) + 1).to_le_bytes();
    match WireConversion::NarrowU64.encode(&host, 0) {
        Err(GpuError::ValueOutOfU32Range {
            buffer_index: 0,
            element_index: 0,
            value: 4_294_967_296,
        }) => {}
        other => panic!("expected a u32 range error, got {other:?}"),
    }
}

#[test]
fn capture_out_of_range_message_names_the_lane_and_the_value() {
    let signed = capture_out_of_range_message(1, 5_000_000_000, 1);
    assert!(signed.starts_with("Runtime error: GPU"), "{signed}");
    assert!(signed.contains("captured scalar 1"), "{signed}");
    assert!(
        signed.contains("5000000000 which exceeds i32 range"),
        "{signed}"
    );
    let unsigned = capture_out_of_range_message(0, -1, 2);
    assert!(
        unsigned.contains("18446744073709551615 which exceeds u32 range"),
        "{unsigned}"
    );
}
