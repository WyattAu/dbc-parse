//! Signal bit coding — the two bit-numbering systems of the DBC format.
//!
//! DBC files number bits within the payload **per byte, LSB-first**: bit `b`
//! lives in byte `b / 8` at in-byte mask `1 << (b % 8)`. That numbering is
//! the same for both byte orders; what differs is *which* bit `start_bit`
//! names and the direction of travel:
//!
//! # Intel (little-endian, `@1`)
//!
//! `start_bit` names the **least significant** bit of the signal; the value
//! is spread over the following `bit_length` bits, ascending, straight
//! through byte boundaries — a plain LSB-first linear field.
//!
//! # Motorola (big-endian, `@0`)
//!
//! `start_bit` names the **most significant** bit of the signal, in the
//! classic "sawtooth" numbering: extraction descends within each byte
//! (MSB → LSB), then jumps from bit 0 of one byte to bit 7 of the next
//! (`+15` in DBC numbering — the sawtooth). This reproduces exactly what
//! `Vector CANdb++`, `cantools`, and the `socketcan-cib` tools do for `@0`
//! signals, including the famous layouts that straddle three bytes.
//!
//! Bits falling outside the buffer read as 0 on extraction
//! ([`extract_raw`] is total); insertion reports the bytes it needs
//! ([`max_bit_position`] → [`crate::DbcError::BufferTooShort`]).

use crate::model::ByteOrder;

/// Extracts `bit_length` raw bits laid out per `byte_order`, MSB-aligned
/// into the low bits of the result.
///
/// Total: positions beyond the buffer contribute zero bits (partial frames
/// decode as if the missing bytes were zero — documented semantics, chosen
/// to match common CAN tooling on underrun payloads).
#[must_use]
pub fn extract_raw(start_bit: u16, bit_length: u8, byte_order: ByteOrder, data: &[u8]) -> u64 {
    match byte_order {
        ByteOrder::Intel => {
            let start = usize::from(start_bit);
            let mut value = 0_u64;
            for i in 0..usize::from(bit_length) {
                let pos = start + i;
                let bit = u64::from((data.get(pos / 8).copied().unwrap_or(0) >> (pos % 8)) & 1);
                value |= bit << i;
            }
            value
        }
        ByteOrder::Motorola => {
            let mut pos = usize::from(start_bit);
            let mut value = 0_u64;
            for _ in 0..usize::from(bit_length) {
                let bit = u64::from((data.get(pos / 8).copied().unwrap_or(0) >> (pos % 8)) & 1);
                value = (value << 1) | bit;
                // Sawtooth: descend within the byte, then jump to the MSB
                // of the next byte (bit 0 → bit 15 in DBC numbering).
                pos = if pos % 8 == 0 { pos + 15 } else { pos - 1 };
            }
            value
        }
    }
}

/// Inserts `value`'s low `bit_length` bits per `byte_order`, the exact
/// inverse of [`extract_raw`].
///
/// # Errors
///
/// [`crate::DbcError::BufferTooShort`] when the buffer cannot hold the
/// signal's highest bit position — insertion never writes out of bounds.
pub fn insert_raw(
    start_bit: u16,
    bit_length: u8,
    byte_order: ByteOrder,
    data: &mut [u8],
    value: u64,
) -> Result<(), crate::DbcError> {
    let need = max_bit_position(start_bit, bit_length, byte_order) / 8 + 1;
    if data.len() < need {
        return Err(crate::DbcError::BufferTooShort {
            need,
            have: data.len(),
        });
    }
    match byte_order {
        ByteOrder::Intel => {
            let start = usize::from(start_bit);
            for i in 0..usize::from(bit_length) {
                let pos = start + i;
                write_bit(data, pos, (value >> i) & 1 == 1);
            }
        }
        ByteOrder::Motorola => {
            let mut pos = usize::from(start_bit);
            for i in (0..usize::from(bit_length)).rev() {
                write_bit(data, pos, (value >> i) & 1 == 1);
                pos = if pos % 8 == 0 { pos + 15 } else { pos - 1 };
            }
        }
    }
    Ok(())
}

/// The highest DBC bit number the signal layout touches.
#[must_use]
pub fn max_bit_position(start_bit: u16, bit_length: u8, byte_order: ByteOrder) -> usize {
    if bit_length == 0 {
        return usize::from(start_bit);
    }
    match byte_order {
        ByteOrder::Intel => usize::from(start_bit) + usize::from(bit_length) - 1,
        ByteOrder::Motorola => {
            let mut pos = usize::from(start_bit);
            let mut max = pos;
            for _ in 1..usize::from(bit_length) {
                pos = if pos % 8 == 0 { pos + 15 } else { pos - 1 };
                max = max.max(pos);
            }
            max
        }
    }
}

/// Sets one bit, LSB-numbered, bounds-checked.
fn write_bit(data: &mut [u8], pos: usize, set: bool) {
    if let Some(byte) = data.get_mut(pos / 8) {
        let mask = 1_u8 << (pos % 8);
        if set {
            *byte |= mask;
        } else {
            *byte &= !mask;
        }
    }
}

/// Sign-extends `raw` (the low `bit_length` bits) to `i64` two's
/// complement.
#[must_use]
pub fn sign_extend(raw: u64, bit_length: u8) -> i64 {
    // u64 → i64 reinterpretation is the point of two's-complement coding.
    #[allow(clippy::cast_possible_wrap)]
    if bit_length == 0 || bit_length >= 64 {
        return raw as i64;
    }
    let bits = u32::from(bit_length);
    let sign_bit = 1_u64 << (bits - 1);
    #[allow(clippy::cast_possible_wrap)]
    if raw & sign_bit != 0 {
        // Fill the high bits above the signal with ones.
        (raw | (!0_u64 << bits)) as i64
    } else {
        raw as i64
    }
}

#[cfg(test)]
mod tests {
    // Tests exercise edge values and index fixed-size buffers directly:
    // unwrap/expect/indexing are lifted at test scope by design.
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    /// Motorola: `39|16@0+` is a big-endian u16 across bytes 4–5.
    #[test]
    fn motorola_big_endian_across_bytes() {
        let data = [0x00, 0x00, 0x00, 0x00, 0xAB, 0xCD, 0x00, 0x00];
        assert_eq!(extract_raw(39, 16, ByteOrder::Motorola, &data), 0xABCD);
    }

    /// Intel: `16|16@1+` is a little-endian u16 across bytes 2–3.
    #[test]
    fn intel_little_endian_across_bytes() {
        let data = [0x00, 0x00, 0xAB, 0xCD, 0x00, 0x00, 0x00, 0x00];
        assert_eq!(extract_raw(16, 16, ByteOrder::Intel, &data), 0xCDAB);
    }

    /// The sawtooth crosses byte boundaries with a +15 jump: a signal
    /// starting at bit 0 descends through byte 1 and finishes in byte 2.
    #[test]
    fn motorola_sawtooth_jump() {
        // start 0 len 12 visits bits: 0, 15..8, 23, 22 (DBC numbering).
        let mut data = [0u8; 8];
        insert_raw(0, 12, ByteOrder::Motorola, &mut data, 0b1101_0100_1001).unwrap();
        // Walk positions: 0, 15, 14, 13, 12, 11, 10, 9, 8, 23, 22, 21.
        // Value 0b110101001001, MSB first along that walk:
        // data[0] bit0 = 1; data[1] = 1010_1001; data[2] top3 = 001.
        assert_eq!(data[0], 0b0000_0001);
        assert_eq!(data[1], 0b1010_1001, "byte 1 holds the 8 mid bits");
        assert_eq!(data[2] >> 5, 0b001, "top 3 bits of byte 2 carry the tail");
        // Round-trip:
        assert_eq!(
            extract_raw(0, 12, ByteOrder::Motorola, &data),
            0b1101_0100_1001
        );
    }

    /// Extraction beyond the buffer reads zero bits (totality).
    #[test]
    fn extract_is_total() {
        let data = [0xFF_u8];
        // Intel: bit 8+ read as zero.
        assert_eq!(extract_raw(0, 16, ByteOrder::Intel, &data), 0x00FF);
        // Motorola from bit 7 down through missing bytes.
        assert_eq!(extract_raw(7, 16, ByteOrder::Motorola, &data), 0xFF00);
        // Fully out of range:
        assert_eq!(extract_raw(8, 8, ByteOrder::Intel, &data), 0);
    }

    /// Insertion refuses to write out of bounds.
    #[test]
    fn insert_bounds_checked() {
        let mut data = [0u8; 2];
        assert_eq!(
            insert_raw(16, 1, ByteOrder::Intel, &mut data, 1),
            Err(crate::DbcError::BufferTooShort { need: 3, have: 2 })
        );
        // Motorola need accounting: start 0, len 12 → touches byte 2.
        let mut four = [0u8; 3];
        assert!(insert_raw(0, 12, ByteOrder::Motorola, &mut four, 0xFFF).is_ok());
        let mut two = [0u8; 2];
        assert_eq!(
            insert_raw(0, 12, ByteOrder::Motorola, &mut two, 0xFFF),
            Err(crate::DbcError::BufferTooShort { need: 3, have: 2 })
        );
    }

    /// `insert_raw` is the exact inverse of `extract_raw`, both orders.
    #[test]
    fn insert_extract_inverse() {
        for &(start, len) in &[
            (0_u16, 1_u8),
            (7, 8),
            (2, 6),
            (39, 16),
            (0, 12),
            (15, 20),
            (7, 64),
        ] {
            for order in [ByteOrder::Motorola, ByteOrder::Intel] {
                let mut data = [0_u8; 9];
                let value: u64 = 0x0123_4567_89AB_CDEF & (u64::MAX >> (64 - len).min(64));
                insert_raw(start, len, order, &mut data, value).unwrap();
                assert_eq!(
                    extract_raw(start, len, order, &data),
                    value,
                    "round-trip failed at start={start} len={len} order={order:?}"
                );
            }
        }
    }

    /// Sign extension of negative and positive raws.
    #[test]
    fn sign_extension() {
        assert_eq!(sign_extend(0x7F, 8), 127);
        assert_eq!(sign_extend(0xFF, 8), -1);
        assert_eq!(sign_extend(0x800, 12), -2048);
        assert_eq!(sign_extend(0x7FF, 12), 2047);
        assert_eq!(sign_extend(1, 1), -1);
        assert_eq!(sign_extend(0, 1), 0);
    }

    /// `max_bit_position` matches the walk for both orders.
    #[test]
    fn max_position() {
        assert_eq!(max_bit_position(16, 16, ByteOrder::Intel), 31);
        assert_eq!(max_bit_position(39, 16, ByteOrder::Motorola), 47);
        assert_eq!(max_bit_position(0, 12, ByteOrder::Motorola), 23);
        assert_eq!(max_bit_position(5, 0, ByteOrder::Intel), 5);
    }
}
