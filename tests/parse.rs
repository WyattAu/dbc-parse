//! Golden tests over real, known-good DBC files (embedded from the
//! cantools test corpus, MIT — see README attribution).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

use dbc_parse::{AttributeObject, AttributeValue, ByteOrder, CanId, Dbc, MultiplexingInfo};

#[test]
fn motohawk_structure() {
    let dbc = Dbc::parse(include_str!("files/motohawk.dbc")).unwrap();
    assert_eq!(dbc.version.as_deref(), Some("1.0"));
    assert_eq!(
        dbc.nodes
            .iter()
            .map(|node| node.name.as_str())
            .collect::<Vec<_>>(),
        vec!["PCM1", "FOO"]
    );
    assert_eq!(dbc.messages.len(), 1);

    let message = &dbc.messages[0];
    assert_eq!(message.id, CanId::Standard(496));
    assert_eq!(message.name, "ExampleMessage");
    assert_eq!(message.dlc, 8);
    assert_eq!(message.sender, "PCM1");
    assert_eq!(message.signals.len(), 3);

    // File order — we preserve it exactly as written.
    assert_eq!(
        message
            .signals
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>(),
        vec!["Temperature", "AverageRadius", "Enable"]
    );

    let temperature = &message.signals[0];
    assert_eq!(temperature.byte_order, ByteOrder::Motorola);
    assert!(temperature.is_signed);
    assert!((temperature.scale - 0.01).abs() < 1e-12);
    assert!((temperature.offset - 250.0).abs() < 1e-12);
    assert!((temperature.min - 229.52).abs() < 1e-9);
    assert!((temperature.max - 270.47).abs() < 1e-9);
    assert_eq!(temperature.unit, "degK");
    assert_eq!(
        temperature.receivers,
        vec!["PCM1".to_string(), "FOO".to_string()]
    );
    assert_eq!(temperature.multiplexing_info, MultiplexingInfo::None);

    let average_radius = &message.signals[1];
    assert_eq!(average_radius.scale, 0.1);
    assert_eq!(average_radius.receivers, vec!["Vector__XXX".to_string()]);

    // The CM_ comment parsed (and, per scope, was skipped without error).
    // BA_DEF_/BA_DEF_DEF_ schemas skipped; motohawk carries no BA_ values.
    assert_eq!(dbc.attributes, &[] as &[dbc_parse::Attribute]);
    // VAL_ table for Enable survived.
    let table = dbc
        .value_tables
        .iter()
        .find(|table| table.signal == "Enable")
        .unwrap();
    assert_eq!(table.message_id, Some(496));
    assert_eq!(
        table.entries,
        vec![(0, "Disabled".to_string()), (1, "Enabled".to_string())]
    );
}

#[test]
fn multiplex_structure() {
    let dbc = Dbc::parse(include_str!("files/multiplex.dbc")).unwrap();
    assert_eq!(dbc.messages.len(), 1);
    let message = &dbc.messages[0];

    // `BO_ 2148676694` = extended flag bit + 0x123456.
    assert_eq!(message.id, CanId::Extended(0x0012_3456));
    assert_eq!(message.name, "Message1");
    assert_eq!(message.sender, "Vector__XXX");
    assert_eq!(message.signals.len(), 12);

    let switch = message
        .signals
        .iter()
        .find(|signal| signal.name == "Multiplexor")
        .unwrap();
    assert_eq!(switch.multiplexing_info, MultiplexingInfo::Multiplexor);
    assert_eq!(switch.start_bit, 2);
    assert_eq!(switch.bit_length, 6);
    assert_eq!(switch.byte_order, ByteOrder::Intel);

    let bit_l = message
        .signals
        .iter()
        .find(|signal| signal.name == "BIT_L")
        .unwrap();
    assert_eq!(bit_l.multiplexing_info, MultiplexingInfo::Value(8));
    let bit_k = message
        .signals
        .iter()
        .find(|signal| signal.name == "BIT_K")
        .unwrap();
    assert_eq!(bit_k.multiplexing_info, MultiplexingInfo::Value(24));

    // Extended multiplexing (`SG_MUL_VAL_`) — four range sets, each
    // 8-8, 16-16, 24-24 over the Multiplexor selector.
    assert_eq!(dbc.extended_multiplexing.len(), 4);
    for mux in &dbc.extended_multiplexing {
        assert_eq!(mux.message_id, 0x0012_3456);
        assert_eq!(mux.selector, "Multiplexor");
        assert_eq!(mux.ranges, vec![(8, 8), (16, 16), (24, 24)]);
    }
    assert!(dbc
        .extended_multiplexing
        .iter()
        .any(|mux| mux.signal == "BIT_L"));

    // Attributes: database-level strings and a message-level enum index.
    assert_eq!(dbc.attributes.len(), 3);
    let bus_type = dbc
        .attributes
        .iter()
        .find(|attribute| attribute.name == "BusType")
        .unwrap();
    assert_eq!(bus_type.object, AttributeObject::Database);
    assert_eq!(bus_type.value, AttributeValue::Str("CAN".to_string()));
    let frame_format = dbc
        .attributes
        .iter()
        .find(|attribute| attribute.name == "VFrameFormat")
        .unwrap();
    assert_eq!(frame_format.object, AttributeObject::Message(0x0012_3456));
    assert_eq!(frame_format.value, AttributeValue::Int(3));
}

/// Multiplexed extraction: only the selected group decodes.
#[test]
fn multiplexed_unpack_selects_the_group() {
    let dbc = Dbc::parse(include_str!("files/multiplex.dbc")).unwrap();
    let message = &dbc.messages[0];

    // mux = 8 (data[0] = 8 << 2 = 0x20), BIT_C at bit 19 (byte 2 bit 3),
    // BIT_L at bit 24 (byte 3 bit 0) — cantools-verified payload.
    let payload = [0x20, 0x00, 0x08, 0x01, 0, 0, 0, 0];
    let values = message.unpack(&payload);
    assert_eq!(values.len(), 5, "m8 group + switch, not the m24 signals");
    let by_name = |name: &str| {
        values
            .iter()
            .find(|(signal, _)| signal.name == name)
            .map(|(_, value)| *value)
    };
    assert_eq!(by_name("Multiplexor"), Some(8.0));
    assert_eq!(by_name("BIT_C"), Some(1.0));
    assert_eq!(by_name("BIT_L"), Some(1.0));
    assert_eq!(by_name("BIT_K"), None, "m24 group must not appear at 8");

    // mux = 24 with the m24 group set — cantools-verified payload.
    let payload = [24 << 2, 0, 0, 0x10, 0x42, 0, 0, 0];
    let values = message.unpack(&payload);
    assert_eq!(
        values.len(),
        8,
        "7 m24 signals + switch, not the m8 signals"
    );
    let by_name = |name: &str| {
        values
            .iter()
            .find(|(signal, _)| signal.name == name)
            .map(|(_, value)| *value)
    };
    assert_eq!(by_name("Multiplexor"), Some(24.0));
    assert_eq!(by_name("BIT_K"), Some(1.0));
    assert_eq!(by_name("BIT_B"), Some(1.0));
    assert_eq!(by_name("BIT_H"), Some(1.0));
    assert_eq!(by_name("BIT_L"), None, "m8 group must not appear at 24");

    // mux = 16: no standard group declares 16 — only the switch decodes.
    // (BIT_L reaches 16 only through the SG_MUL_VAL_ ranges, which
    // Message::unpack deliberately leaves to the caller; see
    // `Dbc::extended_multiplexing`.)
    let payload = [16 << 2, 0, 0, 0x01, 0, 0, 0, 0];
    let values = message.unpack(&payload);
    assert_eq!(values.len(), 1);
    assert_eq!(values[0].0.name, "Multiplexor");
    assert_eq!(values[0].1, 16.0);
}

#[test]
fn syntax_errors_report_lines() {
    let source = "VERSION \"x\"\n\nBO_ nope Bad: 8 X\n";
    let err = Dbc::parse(source).unwrap_err();
    assert_eq!(
        err,
        dbc_parse::DbcError::Syntax {
            line: 3,
            message: "invalid message id: \"nope\"".to_string()
        }
    );
}

#[test]
fn multiplex_token_variants() {
    // `m<N>M` (extended-multiplexor token) parses as the value.
    let dbc = Dbc::parse(
        "BO_ 5 M: 8 N\n SG_ sw M : 0|8@1+ (1,0) [0|0] \"\" N\n SG_ x m3M : 8|8@1+ (1,0) [0|0] \"\" N\n",
    )
    .unwrap();
    let x = dbc.messages[0]
        .signals
        .iter()
        .find(|signal| signal.name == "x")
        .unwrap();
    assert_eq!(x.multiplexing_info, MultiplexingInfo::Value(3));
}

#[test]
fn error_display_covers_every_variant() {
    use dbc_parse::DbcError;
    let cases: Vec<(DbcError, &str)> = vec![
        (
            DbcError::Syntax {
                line: 7,
                message: "boom".to_string(),
            },
            "DBC syntax error at line 7: boom",
        ),
        (
            DbcError::InvalidId {
                raw: 0x8000,
                max: 0x7FF,
            },
            "DBC message id 0x8000 exceeds its width limit 0x7FF",
        ),
        (
            DbcError::ValueOutOfRange {
                value: 300.0,
                min: 0.0,
                max: 255.0,
            },
            "physical value 300 outside the declared range [0|255]",
        ),
        (
            DbcError::RawOutOfRange {
                raw: -1.0,
                bit_length: 8,
            },
            "raw value -1 does not fit a 8-bit signal",
        ),
        (
            DbcError::BufferTooShort { need: 4, have: 2 },
            "buffer of 2 bytes is too short: need 4",
        ),
    ];
    for (error, expected) in cases {
        assert_eq!(error.to_string(), expected);
        assert_ne!(format!("{error:?}"), "");
    }
}
