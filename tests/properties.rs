// Test file: fixed-index access to known-good fixtures is by design, and
// round-trip identity is asserted bit-exactly on purpose (power-of-two
// scales make every intermediate exact in f64).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp
)]
//! proptest: Intel (and Motorola) signal encode/decode round-trips are
//! exact, and `Dbc::parse` is total over arbitrary text. 500 cases per
//! property.

use dbc_parse::{ByteOrder, Dbc, MultiplexingInfo, Signal};
use proptest::prelude::*;

/// Power-of-two scales and integer offsets keep every intermediate value
/// exactly representable in `f64`, so round-trips are bit-exact.
fn scales() -> impl Strategy<Value = f64> {
    prop_oneof![
        Just(0.125),
        Just(0.25),
        Just(0.5),
        Just(1.0),
        Just(2.0),
        Just(4.0),
        Just(1024.0),
    ]
}

fn offsets() -> impl Strategy<Value = f64> {
    prop_oneof![Just(0.0), Just(-128.0), Just(7.0), Just(250.0)]
}

fn build_signal(
    start: u16,
    len: u8,
    order: ByteOrder,
    signed: bool,
    scale: f64,
    offset: f64,
) -> Signal {
    Signal {
        name: "P".to_string(),
        start_bit: start,
        bit_length: len,
        byte_order: order,
        is_signed: signed,
        scale,
        offset,
        // Physical-range checking is covered by the golden tests; round-trip
        // properties run unbounded.
        min: f64::MIN,
        max: f64::MAX,
        unit: String::new(),
        receivers: Vec::new(),
        multiplexing_info: MultiplexingInfo::None,
    }
}

/// Cases for one byte order: layout params plus the physical value computed
/// from an in-representable-range raw. Raws are capped at 2^52 so
/// `raw * scale` (a power of two) is exact in f64 — the crate's documented
/// exactness domain.
fn round_trip_cases(order: ByteOrder) -> impl Strategy<Value = (Signal, f64, [u8; 16])> {
    (0_u16..64).prop_flat_map(move |start| {
        let order = order;
        // start ≤ 63 — no truncation.
        #[allow(clippy::cast_possible_truncation)]
        let max_len = 64 - start as u8;
        (
            1_u8..=max_len,
            any::<bool>(),
            scales(),
            offsets(),
            any::<u64>(),
        )
            .prop_map(move |(len, signed, scale, offset, entropy)| {
                let (_, physical) = if signed {
                    // 1 << min(len-1, 52) is positive — the cast is lossless.
                    #[allow(clippy::cast_sign_loss)]
                    let half = (1_i64 << (len - 1).min(52)) as u64;
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    let signed_raw = (entropy % half) as i64;
                    #[allow(clippy::cast_precision_loss)]
                    let physical = signed_raw as f64 * scale + offset;
                    (signed_raw, physical)
                } else {
                    #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                    let raw = (entropy % (1_u64 << len.min(52))) as i64;
                    #[allow(clippy::cast_precision_loss)]
                    let physical = raw as f64 * scale + offset;
                    (raw, physical)
                };
                let signal = build_signal(start, len, order, signed, scale, offset);
                let mut payload = [0_u8; 16];
                // Intel fits in 8 bytes; Motorola's sawtooth may reach byte
                // (start/8 + ceil(len/8)) ≤ 15 — both inside 16.
                let _ = signal.encode_raw(&mut payload, physical);
                (signal, physical, payload)
            })
    })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(500))]

    /// Intel: encode(v) then decode() is the identity, bit-exact.
    #[test]
    fn intel_round_trip((signal, physical, _) in round_trip_cases(ByteOrder::Intel)) {
        let mut payload = [0_u8; 16];
        signal.encode_raw(&mut payload, physical).unwrap();
        prop_assert_eq!(signal.decode_raw(&payload), physical);
    }

    /// Motorola: same identity through the sawtooth walk.
    #[test]
    fn motorola_round_trip((signal, physical, _) in round_trip_cases(ByteOrder::Motorola)) {
        let mut payload = [0_u8; 16];
        signal.encode_raw(&mut payload, physical).unwrap();
        prop_assert_eq!(signal.decode_raw(&payload), physical);
    }

    /// Intel decode never panics on arbitrary payloads (totality).
    #[test]
    fn intel_decode_is_total(
        (start, len, signed, scale) in (0_u16..64, 1_u8..=64, any::<bool>(), scales()),
        payload in prop::collection::vec(any::<u8>(), 0..8),
    ) {
        let signal = build_signal(start, len, ByteOrder::Intel, signed, scale, 0.0);
        let _ = signal.decode_raw(&payload);
    }

    /// `Dbc::parse` never panics on arbitrary text.
    #[test]
    fn parse_is_total(text in r"[\x20-\x7e\n\t]{0,400}") {
        let _ = Dbc::parse(&text);
    }
}
