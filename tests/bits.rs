//! Golden signal-coding tests — known bit patterns for both byte orders,
//! cross-verified against `cantools` 44.1.0 on the same files.

// Test file: golden fixtures use exact float equality and direct indexing
// by design.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

use dbc_parse::{ByteOrder, Dbc};

/// The motohawk golden payload: `01 a1 02 1a f8 80 00 00`.
const GOLDEN_PAYLOAD: [u8; 8] = [0x01, 0xA1, 0x02, 0x1A, 0xF8, 0x80, 0x00, 0x00];

fn motohawk() -> Dbc {
    Dbc::parse(include_str!("files/motohawk.dbc")).unwrap()
}

#[test]
fn motohawk_temperature_sawtooth_decode() {
    // Temperature: `0|12@0-` — MSB at bit 0, sawtooth across bytes 0, 1, 2.
    // cantools: 242.4 for this payload.
    let dbc = motohawk();
    let temperature = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "Temperature")
        .unwrap();
    assert_eq!(temperature.start_bit, 0);
    assert_eq!(temperature.bit_length, 12);
    assert_eq!(temperature.byte_order, ByteOrder::Motorola);
    assert!(temperature.is_signed);
    assert!((temperature.decode_raw(&GOLDEN_PAYLOAD) - 242.4).abs() < 1e-9);
}

#[test]
fn motohawk_enable_and_average_radius_decode() {
    // Enable: `7|1@0+` → bit 7 of byte 0 = 0. AverageRadius: `6|6@0+`
    // → bits 6..1 of byte 0 = 0. cantools: 0.0 for both.
    let dbc = motohawk();
    let message = &dbc.messages[0];
    let values = message.unpack(&GOLDEN_PAYLOAD);
    assert_eq!(values.len(), 3);
    assert_eq!(values[0].0.name, "Temperature");
    assert_eq!(values[1].0.name, "AverageRadius");
    assert_eq!(values[1].1, 0.0);
    assert_eq!(values[2].0.name, "Enable");
    assert_eq!(values[2].1, 0.0);
}

#[test]
fn motohawk_temperature_sawtooth_encode() {
    // cantools encodes {'Temperature': 266.68} to `00 d0 80 …`:
    // raw 1668 = 0b011010000100 spread along the sawtooth
    // [bit 0] = 0, [bits 15..8] = 1101_0000, [bits 23..21] = 100.
    let dbc = motohawk();
    let temperature = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "Temperature")
        .unwrap();
    let mut payload = [0_u8; 8];
    temperature.encode_raw(&mut payload, 266.68).unwrap();
    assert_eq!(payload[0], 0x00);
    assert_eq!(payload[1], 0xD0);
    assert_eq!(payload[2], 0x80);

    // cantools: 229.52 K is the raw minimum −2048 → `01 00 00 …`
    // (sign bit lands on bit 0 of byte 0).
    let mut payload = [0_u8; 8];
    temperature.encode_raw(&mut payload, 229.52).unwrap();
    assert_eq!(&payload[..3], &[0x01, 0x00, 0x00]);

    // cantools: {'Temperature': 270.47, 'AverageRadius': 5.0, 'Enable': 1}
    // → `e4 ff e0 …`.
    let mut payload = [0_u8; 8];
    for signal in &dbc.messages[0].signals {
        let value = match signal.name.as_str() {
            "Temperature" => 270.47,
            "AverageRadius" => 5.0,
            _ => 1.0,
        };
        signal.encode_raw(&mut payload, value).unwrap();
    }
    assert_eq!(payload[0], 0xE4);
    assert_eq!(payload[1], 0xFF);
    assert_eq!(payload[2], 0xE0);

    // The encoded payload decodes back to the same physical values.
    let values = dbc.messages[0].unpack(&payload);
    assert!((values[0].1 - 270.47).abs() < 1e-9);
    assert!((values[1].1 - 5.0).abs() < 1e-9);
    assert_eq!(values[2].1, 1.0);
}

/// Motorola `39|16@0+` is a big-endian u16 across bytes 4–5.
#[test]
fn motorola_u16_across_bytes() {
    let dbc =
        Dbc::parse("BO_ 100 M: 8 Node\n SG_ S : 39|16@0+ (1,0) [0|65535] \"\" Node\n").unwrap();
    let signal = &dbc.messages[0].signals[0];
    let payload = [0, 0, 0, 0, 0xAB, 0xCD, 0, 0];
    assert_eq!(signal.decode_raw(&payload), f64::from(0xABCD_i32));
    let mut out = [0_u8; 8];
    signal.encode_raw(&mut out, f64::from(0xABCD_i32)).unwrap();
    assert_eq!(&out[..6], &payload[..6]);
}

/// Intel `16|16@1+` is a little-endian u16 across bytes 2–3.
#[test]
fn intel_u16_across_bytes() {
    let dbc =
        Dbc::parse("BO_ 101 M: 8 Node\n SG_ S : 16|16@1+ (1,0) [0|65535] \"\" Node\n").unwrap();
    let signal = &dbc.messages[0].signals[0];
    let payload = [0, 0, 0xAB, 0xCD, 0, 0, 0, 0];
    assert_eq!(signal.decode_raw(&payload), f64::from(0xCDAB_i32));
    let mut out = [0_u8; 8];
    signal.encode_raw(&mut out, f64::from(0xCDAB_i32)).unwrap();
    assert_eq!(out, payload);
}

/// Signed negative values encode via two's complement.
#[test]
fn signed_negative_encoding() {
    let dbc =
        Dbc::parse("BO_ 102 M: 8 Node\n SG_ S : 0|12@0- (1,0) [-2048|2047] \"\" Node\n").unwrap();
    let signal = &dbc.messages[0].signals[0];
    let mut out = [0_u8; 8];
    signal.encode_raw(&mut out, -1.0).unwrap();
    // −1 in 12-bit two's complement = 0xFFF; the walk writes the MSB to
    // bit 0 and ones along bits 15..8, 23..21.
    assert_eq!(out[0] & 1, 1);
    assert_eq!(out[1], 0xFF);
    assert_eq!(out[2] >> 5, 0b111);
    // Bit-exact identity is the contract of encode→decode.
    #[allow(clippy::float_cmp)]
    {
        assert_eq!(signal.decode_raw(&out), -1.0);
    }
}

#[test]
fn encode_rejects_out_of_range() {
    let dbc = motohawk();
    let temperature = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "Temperature")
        .unwrap();
    let mut payload = [0_u8; 8];
    // Declared range [229.52|270.47]:
    let err = temperature.encode_raw(&mut payload, 300.0).unwrap_err();
    assert!(
        matches!(err, dbc_parse::DbcError::ValueOutOfRange { value, min, max }
            if value == 300.0 && min == 229.52 && max == 270.47)
    );
    // Width: physical 2047 maps to raw 4095, which needs 13 bits.
    let dbc2 = Dbc::parse("BO_ 103 M: 8 Node\n SG_ S : 0|12@0- (1,-2048) [-2048|2047] \"\" Node\n")
        .unwrap();
    let signal = &dbc2.messages[0].signals[0];
    let err = signal.encode_raw(&mut payload, 2047.0).unwrap_err();
    assert!(matches!(err, dbc_parse::DbcError::RawOutOfRange { .. }));

    // Non-finite values are always rejected.
    assert!(temperature.encode_raw(&mut payload, f64::NAN).is_err());
    assert!(temperature.encode_raw(&mut payload, f64::INFINITY).is_err());

    // Buffer too short: Temperature touches byte 2.
    let mut short = [0_u8; 2];
    assert!(matches!(
        temperature.encode_raw(&mut short, 250.0),
        Err(dbc_parse::DbcError::BufferTooShort { need: 3, have: 2 })
    ));
}

/// The `[0|0]` range in the file imposes no constraint (min == max).
#[test]
fn empty_range_is_no_constraint() {
    let dbc = motohawk();
    let enable = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "Enable")
        .unwrap();
    let mut payload = [0_u8; 8];
    enable.encode_raw(&mut payload, 1.0).unwrap();
    assert_eq!(payload[0], 0x80);
    assert_eq!(enable.decode_raw(&payload), 1.0);
}

/// Bits beyond the payload read as zero (total decode).
#[test]
fn decode_is_total_on_short_payloads() {
    let dbc = motohawk();
    let temperature = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "Temperature")
        .unwrap();
    // Byte 2 missing → its sawtooth tail bits read as zero:
    // raw = 0b1_10100001_000 = 3336 → −760 (signed) → 242.4 K.
    let value = temperature.decode_raw(&[0x01, 0xA1]);
    assert!((value - 242.4).abs() < 1e-9);
}
