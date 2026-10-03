//! Parser edge branches: every grammar error path, `BA_` object forms,
//! `VAL_TABLE_`, and the model helpers' corners.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::float_cmp,
    clippy::panic
)]

use dbc_parse::{
    AttributeObject, AttributeValue, ByteOrder, CanId, Dbc, DbcError, MultiplexingInfo,
};

fn syntax_line(source: &str) -> usize {
    match Dbc::parse(source).unwrap_err() {
        DbcError::Syntax { line, .. } => line,
        other => panic!("expected Syntax, got {other:?}"),
    }
}

#[test]
fn scalar_expect_failures_are_typed() {
    // expect_word with a punctuation token:
    assert_eq!(syntax_line("BO_ : X: 8 N\n"), 1);
    // expect_punct with a word:
    assert_eq!(syntax_line("BO_ 1 M 8 N\n"), 1);
    // expect_quoted with a word:
    assert_eq!(syntax_line("VERSION x\n"), 1);
    // skip_to_semicolon hitting end of file (EOF reports the line past
    // the trailing newline — the documented approximate behavior):
    assert_eq!(syntax_line("BA_DEF_ BO_ \"A\" INT 0 5\n"), 2);
    // NS_ never terminated by BS_:
    assert_eq!(syntax_line("NS_ :\n\tCM_\n"), 3);
    // Unterminated quoted string:
    let err = Dbc::parse("BO_ 1 M: 8 N\n SG_ A : 0|8@1+ (1,0) [0|0] \"oops\n").unwrap_err();
    assert!(matches!(err, DbcError::Syntax { .. }));
}

#[test]
fn message_header_errors() {
    // dlc above 64:
    assert_eq!(syntax_line("BO_ 1 M: 65 N\n"), 1);
    // sender position carries punctuation:
    assert_eq!(syntax_line("BO_ 1 M: 8 ,\n"), 1);
}

#[test]
fn signal_field_errors() {
    // invalid byte order digit:
    assert_eq!(
        syntax_line("BO_ 1 M: 8 N\n SG_ A : 0|8@2+ (1,0) [0|0] \"\" N\n"),
        2
    );
    // invalid sign flag:
    assert_eq!(
        syntax_line("BO_ 1 M: 8 N\n SG_ A : 0|8@1* (1,0) [0|0] \"\" N\n"),
        2
    );
    // receiver list with a stray separator token:
    assert_eq!(
        syntax_line("BO_ 1 M: 8 N\n SG_ A : 0|8@1+ (1,0) [0|0] \"\" N , |\n"),
        2
    );
    // receiver list ending with a trailing comma:
    assert_eq!(
        syntax_line("BO_ 1 M: 8 N\n SG_ A : 0|8@1+ (1,0) [0|0] \"\" N,\n"),
        2
    );
    // valid receiver lists parse node names in order:
    let dbc =
        Dbc::parse("BU_: N1 N2\nBO_ 1 M: 8 N1\n SG_ A : 0|8@1+ (1,0) [0|0] \"\" N1, N2\n").unwrap();
    assert_eq!(dbc.messages[0].signals[0].receivers, ["N1", "N2"]);
}

#[test]
fn attribute_forms() {
    let dbc = Dbc::parse(
        "BU_: Station1\nBO_ 9 Msg: 8 Station1\n SG_ Sig : 7|8@0+ (1,0) [0|255] \"\" Station1\n\
         BA_ \"NmStationAddress\" BU_ Station1 5;\n\
         BA_ \"GenMsgCycleTime\" BO_ 9 100;\n\
         BA_ \"GenSigSendType\" SG_ 9 Sig 0;\n\
         BA_ \"RelDelay\" 0.5;\n",
    )
    .unwrap();
    let by_name = |name: &str| {
        dbc.attributes
            .iter()
            .find(|attribute| attribute.name == name)
            .unwrap()
    };
    assert_eq!(
        by_name("NmStationAddress").object,
        AttributeObject::Node("Station1".to_string())
    );
    assert_eq!(by_name("NmStationAddress").value, AttributeValue::Int(5));
    assert_eq!(
        by_name("GenMsgCycleTime").object,
        AttributeObject::Message(9)
    );
    assert_eq!(
        by_name("GenSigSendType").object,
        AttributeObject::Signal {
            message_id: 9,
            signal: "Sig".to_string()
        }
    );
    assert_eq!(by_name("GenSigSendType").value, AttributeValue::Int(0));
    // Database-level, float value:
    assert_eq!(by_name("RelDelay").object, AttributeObject::Database);
    assert_eq!(
        by_name("RelDelay").value,
        AttributeValue::Float(0.5),
        "float attribute values take the f64 path"
    );
    // Malformed attribute value:
    assert_eq!(syntax_line("BA_ \"X\" ;\n"), 1);
}

#[test]
fn val_table_and_val_errors() {
    // Global VAL_TABLE_ parses with a name and no message id.
    let dbc = Dbc::parse("VAL_TABLE_ Speeds 0 \"Stop\" 42 \"Warp\" ;\n").unwrap();
    assert_eq!(dbc.value_tables.len(), 1);
    let table = &dbc.value_tables[0];
    assert_eq!(table.name.as_deref(), Some("Speeds"));
    assert_eq!(table.message_id, None);
    assert_eq!(
        table.entries,
        vec![(0, "Stop".to_string()), (42, "Warp".to_string())]
    );

    // VAL_ with a stray token instead of an entry or ';':
    assert_eq!(syntax_line("VAL_ 1 S bogus ;\n"), 1);
    // Empty VAL_ is legal:
    let dbc = Dbc::parse("VAL_ 1 S ;\n").unwrap();
    assert_eq!(dbc.value_tables[0].entries.len(), 0);
}

#[test]
fn sg_mul_val_errors() {
    // Range without '-':
    assert_eq!(syntax_line("SG_MUL_VAL_ 1 S Sw 8 ;\n"), 1);
    // Space-separated ranges parse (leniently, like the sample files'
    // comma form) into one ordered range list:
    let dbc = Dbc::parse("SG_MUL_VAL_ 1 S Sw 8-9 10-11 ;\n").unwrap();
    assert_eq!(dbc.extended_multiplexing[0].ranges, vec![(8, 9), (10, 11)]);
    // Multiple ranges through commas:
    let dbc = Dbc::parse("BO_ 1 M: 8 N\n SG_ Sw M : 0|8@1+ (1,0) [0|0] \"\" N\n SG_ S m8 : 8|8@1+ (1,0) [0|0] \"\" N\nSG_MUL_VAL_ 1 S Sw 2-3, 8-8;\n").unwrap();
    assert_eq!(dbc.extended_multiplexing[0].ranges, vec![(2, 3), (8, 8)]);
}

#[test]
fn model_helpers_corners() {
    // CanId constructors (the DBC-facing alternative to from_dbc_raw):
    assert_eq!(CanId::standard(0x123), Ok(CanId::Standard(0x123)));
    assert!(CanId::standard(0x800).is_err());
    assert!(CanId::standard(u16::MAX).is_err());
    assert_eq!(
        CanId::extended(0x1FFF_FFFF),
        Ok(CanId::Extended(0x1FFF_FFFF))
    );
    assert!(CanId::extended(0x2000_0000).is_err());

    // Accessors:
    let id = CanId::standard(0x7FF).unwrap();
    assert!(id.is_standard());
    assert_eq!(id.raw(), 0x7FF);
    let ext = CanId::extended(0x1).unwrap();
    assert!(!ext.is_standard());
    assert_eq!(ext.raw(), 1);

    // round_half_away: 0.5 fractions round away from zero; > 0.5 rounds up.
    let signal = dbc_parse::Signal {
        name: "S".to_string(),
        start_bit: 7,
        bit_length: 8,
        byte_order: ByteOrder::Motorola,
        is_signed: false,
        scale: 1.0,
        offset: 0.0,
        min: f64::MIN,
        max: f64::MAX,
        unit: String::new(),
        receivers: Vec::new(),
        multiplexing_info: MultiplexingInfo::None,
    };
    let mut payload = [0_u8; 1];
    signal.encode_raw(&mut payload, 1.5).unwrap();
    assert_eq!(
        signal.decode_raw(&payload),
        2.0,
        "half rounds away from zero"
    );
    signal.encode_raw(&mut payload, 1.7).unwrap();
    assert_eq!(signal.decode_raw(&payload), 2.0);
    signal.encode_raw(&mut payload, 1.4).unwrap();
    assert_eq!(signal.decode_raw(&payload), 1.0);

    // floor on huge magnitudes: 1e30 is integral by construction → the raw
    // then fails the width check.
    let err = signal.encode_raw(&mut payload, 1_e30).unwrap_err();
    assert!(matches!(err, DbcError::RawOutOfRange { .. }));

    // two_pow(64): a 64-bit unsigned signal takes the 2^64 bound path.
    let wide = dbc_parse::Signal {
        bit_length: 64,
        is_signed: false,
        start_bit: 0,
        byte_order: ByteOrder::Intel,
        ..signal.clone()
    };
    let mut payload = [0_u8; 8];
    // 2^64 − 4096: the largest f64 below 2^64 (ulp at that magnitude is
    // 4096) — takes the two_pow(64) bound path.
    let top = 18_446_744_073_709_549_568.0_f64;
    wide.encode_raw(&mut payload, top).unwrap();
    assert_eq!(wide.decode_raw(&payload), top);
}
