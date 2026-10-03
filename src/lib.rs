//! DBC file parser — CAN message definitions, signal layout
//! (bit-offset/length/endianness/scaling), value tables, extended
//! multiplexing.
//!
//! `dbc-parse` is the estate's **L0 leaf** for Vector DBC databases: a
//! total, zero-dependency parser for the `BO_`/`SG_`/`CM_`/`VAL_`/`BA_`/
//! `NS_` grammar, plus correct bit coding for both signal byte orders.
//!
//! # The Motorola bit-numbering problem
//!
//! DBC bit numbering runs per byte, LSB-first; the two byte orders differ
//! in what [`Signal::start_bit`] names and the direction of travel:
//!
//! * **Intel (`@1`)** — little-endian, linear: `start_bit` is the LSB, bits
//!   ascend straight through byte boundaries.
//! * **Motorola (`@0`)** — big-endian, the classic "sawtooth":
//!   `start_bit` is the MSB; extraction descends within each byte and
//!   jumps from bit 0 of one byte to bit 7 of the next (`+15` in DBC
//!   numbering). A `39|16@0+` signal is a big-endian `u16` across bytes
//!   4–5; a `0|12@0-` signal straddles three bytes.
//!
//! [`crate::bits`] documents and implements both; the property tests pin
//! encode/decode round-trips against known-good examples.
//!
//! # Example
//!
//! ```
//! use dbc_parse::{ByteOrder, Dbc, MultiplexingInfo};
//!
//! let dbc = Dbc::parse(r#"
//! BO_ 496 ExampleMessage: 8 PCM1
//!  SG_ Temperature : 0|12@0- (0.01,250) [229.52|270.47] "degK" PCM1,FOO
//!  SG_ Enable : 7|1@0+ (1,0) [0|0] "-" Vector__XXX
//! "#)?;
//!
//! let message = &dbc.messages[0];
//! assert_eq!(message.name, "ExampleMessage");
//! let temperature = &message.signals[0];
//! assert_eq!(temperature.byte_order, ByteOrder::Motorola);
//! assert!(temperature.is_signed);
//!
//! // The canonical motohawk pattern: 0x01A1 in bytes 0-1 decodes to 242.4 K.
//! let values = message.unpack(&[0x01, 0xA1, 0x02, 0x1A, 0xF8, 0x80, 0x00, 0x00]);
//! let (name, value) = (&values[0].0.name, values[0].1);
//! assert_eq!(name, "Temperature");
//! assert!((value - 242.4).abs() < 1e-9, "{value}");
//! assert!(matches!(
//!     message.signals[1].multiplexing_info,
//!     MultiplexingInfo::None
//! ));
//! # Ok::<(), dbc_parse::DbcError>(())
//! ```
//!
//! # Scope
//!
//! * Parsed and retained: `VERSION`, `BU_` nodes, `BO_`/`SG_` (incl. `M`,
//!   `m<N>`, `m<N>M`), `VAL_`, `VAL_TABLE_`, `BA_` values, `SG_MUL_VAL_`.
//! * Parsed and skipped: `NS_`, `BS_`, `CM_` comments, `BA_DEF_` /
//!   `BA_DEF_DEF_` schemas, unknown vendor keywords.
//! * `dbc-parse` carries its own minimal [`CanId`] — L0 crates have zero
//!   runtime estate dependencies, so `can-core` cannot be a dependency.
//!
//! # `no_std`
//!
//! `no_std` + `alloc`: builds for embedded targets
//! (`thumbv7em-none-eabihf` is exercised in CI) with zero runtime
//! dependencies of any kind.

#![no_std]
extern crate alloc;

pub mod bits;
mod error;
mod model;
mod parser;

pub use error::DbcError;
pub use model::{
    Attribute, AttributeObject, AttributeValue, ByteOrder, CanId, Dbc, ExtendedMux, Message,
    MultiplexingInfo, Node, Signal, ValueTable, EXTENDED_ID_MAX, STANDARD_ID_MAX,
};
pub use parser::parse;
