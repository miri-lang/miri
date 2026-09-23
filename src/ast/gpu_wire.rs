// SPDX-License-Identifier: Apache-2.0
// Copyright (c) Viacheslav Shynkarenko

//! The GPU wire format: the one rule for how a scalar of each width lives on
//! the device and crosses the host/device boundary.
//!
//! The device has no 8- or 16-bit integers, and the portable (WebGPU) device
//! has no 64-bit integers either, so most integer widths travel in a 32-bit
//! lane: the host widens each narrow element on upload and narrows it back on
//! readback, and a 64-bit value is range-checked into the lane on upload and
//! extended back on readback. `float` is the host's `f64` default and travels
//! as `f32`, the same way `int` travels as `i32`. A float width the source
//! named (`f16`, `f32`, `f64`) keeps that width on the device.
//!
//! The type checker decides admissibility from this table, MIR lowering and
//! the host launch code decide the marshalling from it, and the WGSL backend
//! decides the declared device type from it — so none of them can disagree on
//! a width.

use crate::ast::types::{resolve_element_type_kind, BuiltinCollectionKind, TypeKind};

/// The scalar a value occupies on the device.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceScalar {
    I32,
    U32,
    F16,
    F32,
    F64,
}

impl DeviceScalar {
    /// The Miri spelling of this scalar's type.
    pub fn name(self) -> &'static str {
        match self {
            DeviceScalar::I32 => "i32",
            DeviceScalar::U32 => "u32",
            DeviceScalar::F16 => "f16",
            DeviceScalar::F32 => "f32",
            DeviceScalar::F64 => "f64",
        }
    }

    /// Width of one device element in bytes.
    pub fn byte_width(self) -> u8 {
        match self {
            DeviceScalar::F16 => 2,
            DeviceScalar::I32 | DeviceScalar::U32 | DeviceScalar::F32 => 4,
            DeviceScalar::F64 => 8,
        }
    }
}

/// How the host converts one element between its host width and its device
/// width. The discriminant is the per-buffer code the GPU runtime reads from
/// the launch descriptor, so the numbering is ABI and must not change.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum WireConversion {
    /// Host and device bytes are identical.
    Identity = 0,
    /// Host `i64` to device `i32`: range-checked on upload, sign-extended on
    /// readback.
    NarrowI64 = 1,
    /// Host `u64` to device `u32`: range-checked on upload, zero-extended on
    /// readback.
    NarrowU64 = 2,
    /// Host `i8` to device `i32`: sign-extended on upload, truncated on
    /// readback.
    WidenI8 = 3,
    /// Host `u8` (or `bool`) to device `u32`: zero-extended on upload,
    /// truncated on readback.
    WidenU8 = 4,
    /// Host `i16` to device `i32`: sign-extended on upload, truncated on
    /// readback.
    WidenI16 = 5,
    /// Host `u16` to device `u32`: zero-extended on upload, truncated on
    /// readback.
    WidenU16 = 6,
    /// Host `f64` to device `f32`: rounded on upload, extended on readback.
    DemoteF64 = 7,
}

impl WireConversion {
    /// The code the GPU runtime reads for this conversion.
    pub fn code(self) -> u8 {
        self as u8
    }

    /// The inclusive integer range a value must lie in to survive the upload,
    /// for a conversion that can lose an integer value; `None` when every host
    /// value reaches the device intact (or, for `DemoteF64`, is rounded rather
    /// than refused).
    pub fn checked_range(self) -> Option<(i128, i128)> {
        match self {
            WireConversion::NarrowI64 => Some((i32::MIN.into(), i32::MAX.into())),
            WireConversion::NarrowU64 => Some((0, u32::MAX.into())),
            WireConversion::Identity
            | WireConversion::WidenI8
            | WireConversion::WidenU8
            | WireConversion::WidenI16
            | WireConversion::WidenU16
            | WireConversion::DemoteF64 => None,
        }
    }

    /// Whether the host must convert the bytes rather than copy them.
    pub fn is_identity(self) -> bool {
        self == WireConversion::Identity
    }
}

/// The complete wire description of one scalar: its device scalar, its host
/// width in bytes, and the conversion between them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WireFormat {
    pub device: DeviceScalar,
    pub host_bytes: u8,
    pub conversion: WireConversion,
}

impl WireFormat {
    const fn new(device: DeviceScalar, host_bytes: u8, conversion: WireConversion) -> Self {
        WireFormat {
            device,
            host_bytes,
            conversion,
        }
    }
}

/// The wire format of a numeric scalar — the single table every GPU stage
/// reads. `None` for every non-numeric kind and for the 128-bit integers,
/// which have no device representation.
fn numeric_wire(kind: &TypeKind) -> Option<WireFormat> {
    use DeviceScalar as D;
    use WireConversion as C;
    let format = match kind {
        TypeKind::Int | TypeKind::I64 => WireFormat::new(D::I32, 8, C::NarrowI64),
        TypeKind::I32 => WireFormat::new(D::I32, 4, C::Identity),
        TypeKind::I16 => WireFormat::new(D::I32, 2, C::WidenI16),
        TypeKind::I8 => WireFormat::new(D::I32, 1, C::WidenI8),
        TypeKind::U64 => WireFormat::new(D::U32, 8, C::NarrowU64),
        TypeKind::U32 => WireFormat::new(D::U32, 4, C::Identity),
        TypeKind::U16 => WireFormat::new(D::U32, 2, C::WidenU16),
        TypeKind::U8 => WireFormat::new(D::U32, 1, C::WidenU8),
        TypeKind::Float => WireFormat::new(D::F32, 8, C::DemoteF64),
        TypeKind::F32 => WireFormat::new(D::F32, 4, C::Identity),
        TypeKind::F16 => WireFormat::new(D::F16, 2, C::Identity),
        TypeKind::F64 => WireFormat::new(D::F64, 8, C::Identity),
        TypeKind::I128
        | TypeKind::U128
        | TypeKind::Boolean
        | TypeKind::String
        | TypeKind::Void
        | TypeKind::Error
        | TypeKind::Identifier
        | TypeKind::RawPtr
        | TypeKind::List(_)
        | TypeKind::Array(_, _)
        | TypeKind::Map(_, _)
        | TypeKind::Tuple(_)
        | TypeKind::Set(_)
        | TypeKind::Result(_, _)
        | TypeKind::Future(_)
        | TypeKind::Function(_)
        | TypeKind::Generic(_, _, _)
        | TypeKind::Custom(_, _)
        | TypeKind::Meta(_)
        | TypeKind::Option(_)
        | TypeKind::Linear(_) => return None,
    };
    Some(format)
}

/// The scalar a numeric value of `kind` occupies anywhere on the device — a
/// kernel local, a buffer element, or a captured scalar. `None` for a
/// non-numeric kind.
pub fn device_scalar(kind: &TypeKind) -> Option<DeviceScalar> {
    numeric_wire(kind).map(|format| format.device)
}

/// The wire format of a storage-buffer element of type `kind`, or `None` when
/// such an element cannot live in a device buffer. `bool` is refused because
/// WGSL forbids it in `var<storage>`.
pub fn buffer_element_wire(kind: &TypeKind) -> Option<WireFormat> {
    numeric_wire(kind)
}

/// The wire format of a host scalar captured into a kernel, or `None` when it
/// cannot be captured. Captures are pooled into one uniform block of 32-bit
/// fields, so a capture must travel in a 32-bit lane: `f16` and `f64` keep
/// their named widths on the device and are refused, and `bool` travels as a
/// `u32` the kernel reads back as a `bool`.
pub fn scalar_capture_wire(kind: &TypeKind) -> Option<WireFormat> {
    if matches!(kind, TypeKind::Boolean) {
        return Some(WireFormat::new(
            DeviceScalar::U32,
            1,
            WireConversion::WidenU8,
        ));
    }
    numeric_wire(kind).filter(|format| format.device.byte_width() == 4)
}

/// The conversion every element of a buffer-backed collection of type
/// `collection` (`Array<T, N>`, `List<T>`, or their resolved envelopes) needs
/// between host and device. `Identity` for an element with no scalar wire
/// format of its own — a `VecN` or `Atomic` element, which is admitted only at
/// a 32-bit component width the host already stores.
pub fn buffer_conversion(collection: &TypeKind) -> WireConversion {
    collection_element_kind(collection)
        .and_then(|element| buffer_element_wire(&element))
        .map_or(WireConversion::Identity, |format| format.conversion)
}

/// How many low bits of its 32-bit lane a device integer of `kind` holds, for
/// an integer narrower than the lane (`i8`/`u8`: 8, `i16`/`u16`: 16); `None`
/// for every kind that fills its lane or is not an integer. The device
/// computes such a value at the full lane width, so every result has to be
/// brought back to these bits — sign-extended for a signed kind,
/// zero-extended for an unsigned one — before it is read.
pub fn sub_lane_bits(kind: &TypeKind) -> Option<u32> {
    match numeric_wire(kind)?.conversion {
        WireConversion::WidenI8 | WireConversion::WidenU8 => Some(8),
        WireConversion::WidenI16 | WireConversion::WidenU16 => Some(16),
        WireConversion::Identity
        | WireConversion::NarrowI64
        | WireConversion::NarrowU64
        | WireConversion::DemoteF64 => None,
    }
}

/// The element kind of a buffer-backed collection type, or `None` for any
/// other type.
pub fn collection_element_kind(collection: &TypeKind) -> Option<TypeKind> {
    if let TypeKind::Array(element, _) | TypeKind::List(element) = collection {
        return resolve_element_type_kind(element);
    }
    if let TypeKind::Custom(name, Some(args)) = collection {
        if matches!(
            BuiltinCollectionKind::from_name(name),
            Some(BuiltinCollectionKind::Array | BuiltinCollectionKind::List)
        ) {
            return args.first().and_then(resolve_element_type_kind);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const INTEGER_KINDS: [TypeKind; 9] = [
        TypeKind::Int,
        TypeKind::I8,
        TypeKind::I16,
        TypeKind::I32,
        TypeKind::I64,
        TypeKind::U8,
        TypeKind::U16,
        TypeKind::U32,
        TypeKind::U64,
    ];

    fn buffer(kind: &TypeKind) -> WireFormat {
        buffer_element_wire(kind).unwrap_or_else(|| panic!("{kind:?} should be a buffer element"))
    }

    #[test]
    fn test_every_integer_width_travels_in_a_32_bit_lane() {
        for kind in INTEGER_KINDS {
            assert_eq!(buffer(&kind).device.byte_width(), 4, "{kind:?}");
        }
    }

    #[test]
    fn test_integer_lane_signedness_follows_the_source_type() {
        for kind in [
            TypeKind::Int,
            TypeKind::I8,
            TypeKind::I16,
            TypeKind::I32,
            TypeKind::I64,
        ] {
            assert_eq!(buffer(&kind).device, DeviceScalar::I32, "{kind:?}");
        }
        for kind in [TypeKind::U8, TypeKind::U16, TypeKind::U32, TypeKind::U64] {
            assert_eq!(buffer(&kind).device, DeviceScalar::U32, "{kind:?}");
        }
    }

    #[test]
    fn test_host_width_is_the_width_the_source_named() {
        let expected = [
            (TypeKind::Int, 8),
            (TypeKind::I8, 1),
            (TypeKind::I16, 2),
            (TypeKind::I32, 4),
            (TypeKind::I64, 8),
            (TypeKind::U8, 1),
            (TypeKind::U16, 2),
            (TypeKind::U32, 4),
            (TypeKind::U64, 8),
            (TypeKind::Float, 8),
            (TypeKind::F16, 2),
            (TypeKind::F32, 4),
            (TypeKind::F64, 8),
        ];
        for (kind, bytes) in expected {
            assert_eq!(buffer(&kind).host_bytes, bytes, "{kind:?}");
        }
    }

    /// The conversion is the identity exactly when host and device widths
    /// agree, so no element is ever copied at the wrong stride.
    #[test]
    fn test_conversion_is_identity_exactly_when_the_widths_agree() {
        for kind in INTEGER_KINDS.into_iter().chain([
            TypeKind::Float,
            TypeKind::F16,
            TypeKind::F32,
            TypeKind::F64,
        ]) {
            let format = buffer(&kind);
            assert_eq!(
                format.conversion.is_identity(),
                format.host_bytes == format.device.byte_width(),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn test_only_64_bit_integers_are_range_checked() {
        assert_eq!(
            buffer(&TypeKind::I64).conversion.checked_range(),
            Some((i128::from(i32::MIN), i128::from(i32::MAX)))
        );
        assert_eq!(
            buffer(&TypeKind::U64).conversion.checked_range(),
            Some((0, i128::from(u32::MAX)))
        );
        for kind in [TypeKind::I8, TypeKind::U16, TypeKind::I32, TypeKind::Float] {
            assert_eq!(buffer(&kind).conversion.checked_range(), None, "{kind:?}");
        }
    }

    #[test]
    fn test_named_float_widths_keep_their_width_and_float_travels_as_f32() {
        assert_eq!(buffer(&TypeKind::Float).device, DeviceScalar::F32);
        assert_eq!(buffer(&TypeKind::F16).device, DeviceScalar::F16);
        assert_eq!(buffer(&TypeKind::F64).device, DeviceScalar::F64);
    }

    #[test]
    fn test_bool_is_a_capture_but_not_a_buffer_element() {
        assert_eq!(buffer_element_wire(&TypeKind::Boolean), None);
        let capture = scalar_capture_wire(&TypeKind::Boolean)
            .unwrap_or_else(|| panic!("bool should be capturable"));
        assert_eq!(capture.device, DeviceScalar::U32);
        assert_eq!(capture.conversion, WireConversion::WidenU8);
    }

    #[test]
    fn test_captures_refuse_widths_that_are_not_a_32_bit_lane() {
        for kind in [TypeKind::F16, TypeKind::F64, TypeKind::I128, TypeKind::U128] {
            assert_eq!(scalar_capture_wire(&kind), None, "{kind:?}");
        }
        for kind in INTEGER_KINDS
            .into_iter()
            .chain([TypeKind::Float, TypeKind::F32])
        {
            assert_eq!(
                scalar_capture_wire(&kind),
                buffer_element_wire(&kind),
                "{kind:?}"
            );
        }
    }

    #[test]
    fn test_non_numeric_kinds_have_no_wire_format() {
        for kind in [
            TypeKind::String,
            TypeKind::Void,
            TypeKind::I128,
            TypeKind::U128,
        ] {
            assert_eq!(buffer_element_wire(&kind), None, "{kind:?}");
            assert_eq!(device_scalar(&kind), None, "{kind:?}");
        }
    }

    #[test]
    fn test_buffer_conversion_reads_the_collection_element() {
        use crate::ast::expression::{Expression, ExpressionKind};
        use crate::ast::types::Type;
        use crate::error::syntax::Span;
        let element = |kind: TypeKind| {
            let ty = Type::new(kind, Span::new(0, 0));
            Box::new(Expression::new(
                0,
                ExpressionKind::Type(Box::new(ty), false),
                Span::new(0, 0),
            ))
        };
        assert_eq!(
            buffer_conversion(&TypeKind::List(element(TypeKind::I16))),
            WireConversion::WidenI16
        );
        let list_envelope = TypeKind::Custom(
            BuiltinCollectionKind::List.name().to_string(),
            Some(vec![*element(TypeKind::I64)]),
        );
        assert_eq!(buffer_conversion(&list_envelope), WireConversion::NarrowI64);
        assert_eq!(
            buffer_conversion(&TypeKind::List(element(TypeKind::F32))),
            WireConversion::Identity
        );
        assert_eq!(buffer_conversion(&TypeKind::I64), WireConversion::Identity);
    }

    #[test]
    fn test_only_sub_word_integers_occupy_part_of_their_lane() {
        assert_eq!(sub_lane_bits(&TypeKind::I8), Some(8));
        assert_eq!(sub_lane_bits(&TypeKind::U8), Some(8));
        assert_eq!(sub_lane_bits(&TypeKind::I16), Some(16));
        assert_eq!(sub_lane_bits(&TypeKind::U16), Some(16));
        for kind in [
            TypeKind::Int,
            TypeKind::I32,
            TypeKind::I64,
            TypeKind::U32,
            TypeKind::U64,
            TypeKind::Float,
            TypeKind::F16,
            TypeKind::Boolean,
        ] {
            assert_eq!(sub_lane_bits(&kind), None, "{kind:?}");
        }
    }

    /// The runtime decodes these codes; renumbering one would silently apply
    /// the wrong conversion to every buffer that uses it.
    #[test]
    fn test_conversion_codes_are_the_runtime_abi() {
        assert_eq!(WireConversion::Identity.code(), 0);
        assert_eq!(WireConversion::NarrowI64.code(), 1);
        assert_eq!(WireConversion::NarrowU64.code(), 2);
        assert_eq!(WireConversion::WidenI8.code(), 3);
        assert_eq!(WireConversion::WidenU8.code(), 4);
        assert_eq!(WireConversion::WidenI16.code(), 5);
        assert_eq!(WireConversion::WidenU16.code(), 6);
        assert_eq!(WireConversion::DemoteF64.code(), 7);
    }
}
