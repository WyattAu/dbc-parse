//! The parsed database model — nodes, messages, signals, value tables,
//! attributes.

use crate::bits;
use crate::error::DbcError;
use alloc::string::String;
use alloc::vec::Vec;

/// `2^n` for `n` in `0..=64` — `core` has no `f64::powi`.
#[must_use]
fn two_pow(n: u8) -> f64 {
    if n == 64 {
        // 2^64, exactly representable as a power of two.
        18_446_744_073_709_551_616.0
    } else {
        // (1 << n) ≤ 2^63 — exactly representable as f64.
        #[allow(clippy::cast_precision_loss)]
        {
            (1_u64 << n) as f64
        }
    }
}

/// `floor` — `core` has no `f64::floor`. Exact for finite inputs; values
/// with magnitude ≥ 2^53 are integral by construction.
#[must_use]
fn floor(x: f64) -> f64 {
    if !x.is_finite() || x.abs() >= 9_007_199_254_740_992.0 {
        return x;
    }
    // |x| < 2^53 fits i64 exactly; the cast truncates toward zero.
    #[allow(clippy::cast_possible_truncation, clippy::cast_precision_loss)]
    let t = x as i64 as f64;
    // Exact equality is the point: `t == x` detects already-integral values.
    #[allow(clippy::float_cmp)]
    if x >= 0.0 || t == x {
        t
    } else {
        t - 1.0
    }
}

/// Half-away-from-zero rounding — `core` has no `f64::round`.
#[must_use]
fn round_half_away(x: f64) -> f64 {
    let floor = floor(x);
    let diff = x - floor;
    if diff > 0.5 {
        floor + 1.0
    } else if diff < 0.5 || x < 0.0 {
        floor
    } else {
        floor + 1.0
    }
}

/// Largest 11-bit standard identifier.
pub const STANDARD_ID_MAX: u32 = 0x7FF;
/// Largest 29-bit extended identifier.
pub const EXTENDED_ID_MAX: u32 = 0x1FFF_FFFF;

/// A CAN identifier: 11-bit (standard) or 29-bit (extended).
///
/// `dbc-parse` carries its own minimal `CanId` rather than depending on the
/// estate's `can-core`: L0 crates have zero runtime estate dependencies
/// (`docs/layers.md`, engineering-standards). The shape mirrors `can-core`
/// so a future higher-layer bridge can convert with `match`.
///
/// The tuple variants are public, so — as in `can-core` — the crate's
/// contract is defense in depth: the validating constructors reject
/// out-of-range values and the parser never produces an out-of-range id.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CanId {
    /// 11-bit standard identifier, `0x000..=0x7FF` (a `u11` in a `u16`).
    Standard(u16),
    /// 29-bit extended identifier, `0x000..=0x1FFF_FFFF` (a `u29` in a
    /// `u32`).
    Extended(u32),
}

impl CanId {
    /// Validates and wraps an 11-bit standard identifier.
    ///
    /// # Errors
    ///
    /// [`DbcError::InvalidId`] when `raw > 0x7FF`.
    pub fn standard(raw: u16) -> Result<Self, DbcError> {
        if u32::from(raw) > STANDARD_ID_MAX {
            return Err(DbcError::InvalidId {
                max: STANDARD_ID_MAX,
                raw: u32::from(raw),
            });
        }
        Ok(Self::Standard(raw))
    }

    /// Validates and wraps a 29-bit extended identifier.
    ///
    /// # Errors
    ///
    /// [`DbcError::InvalidId`] when `raw > 0x1FFF_FFFF`.
    pub fn extended(raw: u32) -> Result<Self, DbcError> {
        if raw > EXTENDED_ID_MAX {
            return Err(DbcError::InvalidId {
                max: EXTENDED_ID_MAX,
                raw,
            });
        }
        Ok(Self::Extended(raw))
    }

    /// Interprets a DBC `BO_` id word: bit 31 (`0x8000_0000`) marks an
    /// extended frame and is stripped; the remaining 29 bits are the id.
    /// A plain id that exceeds the 11-bit standard width is rejected —
    /// the DBC format requires bit 31 for extended frames.
    ///
    /// # Errors
    ///
    /// [`DbcError::InvalidId`] for error/flag bits below bit 31 or an
    /// over-wide standard id.
    pub fn from_dbc_raw(raw: u32) -> Result<Self, DbcError> {
        if raw & 0x8000_0000 != 0 {
            Self::extended(raw & 0x1FFF_FFFF)
        } else if raw > STANDARD_ID_MAX {
            Err(DbcError::InvalidId {
                max: STANDARD_ID_MAX,
                raw,
            })
        } else {
            // raw ≤ 0x7FF — no truncation.
            #[allow(clippy::cast_possible_truncation)]
            Ok(Self::Standard(raw as u16))
        }
    }

    /// `true` for the 11-bit standard format.
    #[must_use]
    pub const fn is_standard(&self) -> bool {
        matches!(self, Self::Standard(_))
    }

    /// The identifier value, widened to `u32`.
    #[must_use]
    pub const fn raw(&self) -> u32 {
        match *self {
            // u16 → u32 widening is lossless.
            Self::Standard(v) => v as u32,
            Self::Extended(v) => v,
        }
    }
}

/// A node declared in the `BU_` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Node name, e.g. `PCM1`.
    pub name: String,
}

/// Byte order of a signal's bit layout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ByteOrder {
    /// Big-endian ("Motorola"): `start_bit` names the MSB, in the classic
    /// sawtooth DBC numbering (`@0` in the grammar).
    Motorola,
    /// Little-endian ("Intel"): `start_bit` names the LSB, laid out linearly
    /// ascending (`@1` in the grammar).
    Intel,
}

/// Multiplexing role of a signal within its message.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MultiplexingInfo {
    /// A plain signal, always present.
    None,
    /// The multiplexor switch (`M`): its value selects which multiplexed
    /// signals are present in a frame.
    Multiplexor,
    /// A multiplexed signal (`m<N>`): present only when the message's
    /// multiplexor equals `N`.
    Value(i32),
}

/// One signal of a [`Message`] — its layout, scaling, and metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct Signal {
    /// Signal name, e.g. `Temperature`.
    pub name: String,
    /// Start bit, in DBC bit numbering: the MSB for [`ByteOrder::Motorola`],
    /// the LSB for [`ByteOrder::Intel`] (see [`crate::bits`]). Up to 511 —
    /// CAN FD layouts reach into byte 64.
    pub start_bit: u16,
    /// Signal width in bits (1–64).
    pub bit_length: u8,
    /// Bit layout order.
    pub byte_order: ByteOrder,
    /// Two's-complement signed interpretation of the raw bits.
    pub is_signed: bool,
    /// Physical value = `raw * scale + offset`.
    pub scale: f64,
    /// Physical value = `raw * scale + offset`.
    pub offset: f64,
    /// Declared physical minimum (from `[min|max]`).
    pub min: f64,
    /// Declared physical maximum (from `[min|max]`).
    pub max: f64,
    /// Unit string, e.g. `"degK"` (empty when the file carries `"-"` or `""`).
    pub unit: String,
    /// Receiving node names, in file order (`Vector__XXX` preserved as-is).
    pub receivers: Vec<String>,
    /// Multiplexing role within the message.
    pub multiplexing_info: MultiplexingInfo,
}

impl Signal {
    /// Decodes the signal's physical value from a CAN frame payload: the
    /// raw bits are extracted per the layout, sign-extended when signed,
    /// then scaled (`raw * scale + offset`).
    ///
    /// Total: bits beyond the buffer read as zero (partial payloads decode
    /// as if zero-filled — see [`crate::bits::extract_raw`]).
    #[must_use]
    pub fn decode_raw(&self, data: &[u8]) -> f64 {
        let raw = bits::extract_raw(self.start_bit, self.bit_length, self.byte_order, data);
        // f64 is the API's value domain — precision loss for raws above
        // 2^53 is inherent and documented.
        #[allow(clippy::cast_precision_loss)]
        let as_int = if self.is_signed {
            bits::sign_extend(raw, self.bit_length) as f64
        } else {
            raw as f64
        };
        as_int * self.scale + self.offset
    }

    /// Encodes a physical value into a CAN frame payload: scaled back to
    /// raw (`round((value - offset) / scale)`, half away from zero),
    /// range- and width-checked, then written bit by bit.
    ///
    /// The `[min|max]` range is enforced only when `min < max` — files that
    /// declare an empty range (`[0|0]`, as in `motohawk.dbc`'s `Enable`)
    /// impose no constraint.
    ///
    /// # Errors
    ///
    /// [`DbcError::ValueOutOfRange`] for non-finite values or declared-range
    /// violations, [`DbcError::RawOutOfRange`] when the scaled raw does not
    /// fit the bit width, [`DbcError::BufferTooShort`] when the buffer
    /// cannot hold the layout.
    pub fn encode_raw(&self, data: &mut [u8], value: f64) -> Result<(), DbcError> {
        if !value.is_finite() || (self.min < self.max && !(value >= self.min && value <= self.max))
        {
            return Err(DbcError::ValueOutOfRange {
                value,
                min: self.min,
                max: self.max,
            });
        }
        let raw = round_half_away((value - self.offset) / self.scale);
        let width = self.bit_length;
        let fits = if self.is_signed {
            let half = two_pow(width - 1);
            raw >= -half && raw < half
        } else {
            raw >= 0.0 && raw < two_pow(width)
        };
        // i64::MIN = −2^63 is a power of two — exact in f64.
        #[allow(clippy::cast_precision_loss)]
        let i64_min = i64::MIN as f64;
        // Signed raws must land inside i64 for the two's-complement cast;
        // unsigned raws are already bounded by `fits` (< 2^width ≤ 2^64).
        let needs_i64 = self.is_signed && (raw < i64_min || raw >= two_pow(63));
        if !fits || needs_i64 {
            return Err(DbcError::RawOutOfRange {
                raw,
                bit_length: self.bit_length,
            });
        }
        // In range per the checks above; two's complement for signed.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let raw_bits = if self.is_signed {
            raw as i64 as u64
        } else {
            raw as u64
        };
        bits::insert_raw(
            self.start_bit,
            self.bit_length,
            self.byte_order,
            data,
            raw_bits,
        )
    }
}

/// A CAN message declaration (`BO_`), with its signals (`SG_`).
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    /// The message identifier.
    pub id: CanId,
    /// Message name, e.g. `ExampleMessage`.
    pub name: String,
    /// Declared payload length in bytes (the `dlc` column).
    pub dlc: u8,
    /// Transmitting node name (`Vector__XXX` when unspecified).
    pub sender: String,
    /// The message's signals, in file order.
    pub signals: Vec<Signal>,
}

impl Message {
    /// Decodes every applicable signal of the frame: `(signal, physical
    /// value)` pairs, in file order.
    ///
    /// Multiplexed messages decode per the multiplexor: the switch signal
    /// (`M`) and plain signals are always included; `m<N>` signals only
    /// when the decoded switch equals `N`. Signals gated by *extended*
    /// multiplexing ranges (`SG_MUL_VAL_`) are not filtered here — they
    /// need the database-level ranges in [`Dbc::extended_multiplexing`].
    #[must_use]
    pub fn unpack(&self, data: &[u8]) -> Vec<(Signal, f64)> {
        let selector = self
            .signals
            .iter()
            .find(|signal| signal.multiplexing_info == MultiplexingInfo::Multiplexor)
            .map(|switch| {
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let raw = round_half_away(switch.decode_raw(data)) as i64;
                raw
            });
        self.signals
            .iter()
            .filter(|signal| match (signal.multiplexing_info, selector) {
                (MultiplexingInfo::Value(tag), Some(selected)) => i64::from(tag) == selected,
                _ => true,
            })
            .map(|signal| (signal.clone(), signal.decode_raw(data)))
            .collect()
    }
}

/// A value table: raw value → label mappings from `VAL_` (message-scoped)
/// or `VAL_TABLE_` (global, named).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValueTable {
    /// Table name for `VAL_TABLE_` declarations; `None` for message-scoped
    /// `VAL_` entries.
    pub name: Option<String>,
    /// Message id for `VAL_` declarations; `None` for `VAL_TABLE_`.
    pub message_id: Option<u32>,
    /// Signal name the table annotates (unused for `VAL_TABLE_`).
    pub signal: String,
    /// `(raw value, label)` pairs, in file order.
    pub entries: Vec<(i64, String)>,
}

/// The object an attribute value attaches to (`BA_`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AttributeObject {
    /// Database-level attribute (`BA_ "name" value;`).
    Database,
    /// Node-level (`BA_ "name" BU_ node value;`).
    Node(String),
    /// Message-level (`BA_ "name" BO_ id value;`).
    Message(u32),
    /// Signal-level (`BA_ "name" SG_ id signal value;`).
    Signal {
        /// Owning message id.
        message_id: u32,
        /// Signal name.
        signal: String,
    },
}

/// An attribute value as carried by `BA_` lines.
#[derive(Debug, Clone, PartialEq)]
pub enum AttributeValue {
    /// Integer (or enum index) value.
    Int(i64),
    /// Float value.
    Float(f64),
    /// String (or enum label) value.
    Str(String),
}

/// One `BA_` attribute assignment.
#[derive(Debug, Clone, PartialEq)]
pub struct Attribute {
    /// Attribute name, e.g. `GenMsgCycleTime`.
    pub name: String,
    /// The object the value attaches to.
    pub object: AttributeObject,
    /// The assigned value.
    pub value: AttributeValue,
}

/// An extended-multiplexing range set (`SG_MUL_VAL_`): a signal is present
/// when its selector takes any value in any of the `(from, to)` inclusive
/// ranges.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExtendedMux {
    /// Owning message id.
    pub message_id: u32,
    /// The gated signal.
    pub signal: String,
    /// The selector signal (`Multiplexor` in the sample files).
    pub selector: String,
    /// Inclusive `(from, to)` value ranges, in file order.
    pub ranges: Vec<(i32, i32)>,
}

/// A parsed DBC database.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Dbc {
    /// `VERSION "..."` string, when present.
    pub version: Option<String>,
    /// Nodes from the `BU_` section.
    pub nodes: Vec<Node>,
    /// Messages (`BO_`) with their signals (`SG_`).
    pub messages: Vec<Message>,
    /// Value tables from `VAL_` and `VAL_TABLE_`.
    pub value_tables: Vec<ValueTable>,
    /// Attribute assignments from `BA_` (`BA_DEF_` schemas and
    /// `BA_DEF_DEF_` defaults are validated-and-skipped, not retained).
    pub attributes: Vec<Attribute>,
    /// Extended-multiplexing range sets from `SG_MUL_VAL_`.
    pub extended_multiplexing: Vec<ExtendedMux>,
}

impl Dbc {
    /// Parses a DBC document — the primary entry point.
    ///
    /// # Errors
    ///
    /// [`DbcError::Syntax`] for malformed known constructs (with 1-based
    /// line numbers), [`DbcError::InvalidId`] for unrepresentable message
    /// ids. Unknown vendor keywords are skipped, never rejected.
    #[allow(clippy::should_implement_trait)] // the format's verb, and the API
    pub fn parse(input: &str) -> Result<Self, DbcError> {
        crate::parser::parse(input)
    }
}
