//! Typed, exhaustive error taxonomy for every operation in the crate.
//!
//! The enum is deliberately **exhaustive** (no `#[non_exhaustive]`): callers
//! are expected to match every variant, and a new variant is a semver-minor
//! event by design — the same policy as `wire-kit`'s `WireError`. The parser
//! never panics on malformed input: every failure resolves to one of these
//! variants.

use alloc::string::String;
use core::fmt;

/// The single crate-level error for parsing and signal coding.
#[derive(Debug, Clone, PartialEq)]
pub enum DbcError {
    /// A grammar violation in the DBC text. `line` is the 1-based line the
    /// violation was detected on (approximate for multi-line constructs);
    /// `message` describes what was expected.
    Syntax {
        /// 1-based source line of the violation.
        line: usize,
        /// What was expected versus what was found.
        message: String,
    },
    /// A `BO_` identifier is not representable: DBC marks extended frames
    /// with bit 31 (`0x8000_0000`) and caps the payload at 29 bits; plain
    /// identifiers must fit the 11-bit standard width.
    InvalidId {
        /// The offending raw id word from the file.
        raw: u32,
        /// Largest value the identifier width permits.
        max: u32,
    },
    /// A physical value lies outside the signal's declared `[min|max]`
    /// range (or is not finite — NaN and infinities are always rejected).
    ValueOutOfRange {
        /// The offending physical value.
        value: f64,
        /// Declared minimum.
        min: f64,
        /// Declared maximum.
        max: f64,
    },
    /// The scaled raw value does not fit the signal's bit width and
    /// signedness.
    RawOutOfRange {
        /// The computed raw value (pre-truncation).
        raw: f64,
        /// The signal's bit length.
        bit_length: u8,
    },
    /// The buffer is shorter than the signal layout requires.
    BufferTooShort {
        /// Number of bytes the layout requires.
        need: usize,
        /// Number of bytes actually available.
        have: usize,
    },
}

impl fmt::Display for DbcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Syntax { line, message } => {
                write!(f, "DBC syntax error at line {line}: {message}")
            }
            Self::InvalidId { raw, max } => {
                write!(
                    f,
                    "DBC message id 0x{raw:X} exceeds its width limit 0x{max:X}"
                )
            }
            Self::ValueOutOfRange { value, min, max } => {
                write!(
                    f,
                    "physical value {value} outside the declared range [{min}|{max}]"
                )
            }
            Self::RawOutOfRange { raw, bit_length } => {
                write!(f, "raw value {raw} does not fit a {bit_length}-bit signal")
            }
            Self::BufferTooShort { need, have } => {
                write!(f, "buffer of {have} bytes is too short: need {need}")
            }
        }
    }
}

impl core::error::Error for DbcError {}
