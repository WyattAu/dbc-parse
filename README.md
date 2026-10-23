# dbc-parse

DBC file parser: CAN message definitions, signal layout
(bit-offset/length/endianness/scaling), value tables, extended multiplexing.

**Layer:** L0: leaf · **Estate deps:** zero (own minimal `CanId`) ·
**Runtime deps:** zero · `no_std` + `alloc` (CI exercises
`thumbv7em-none-eabihf`).

## What's here

| Module | Contents |
|---|---|
| `model` | `Dbc` (`Dbc::parse`), `Message::unpack`, `Signal::decode_raw`/`encode_raw`, `Node`, `ValueTable`, `Attribute`, `ExtendedMux`, `CanId` |
| `bits` | The two DBC bit-numbering systems: Motorola (big-endian, `@0`: `start_bit` is the MSB, sawtooth walk) and Intel (little-endian, `@1`: `start_bit` is the LSB, linear) |
| `parser` | The Vector DBC grammar: `VERSION`, `NS_`, `BS_`, `BU_`, `BO_`, `SG_` (`M`/`m<N>`/`m<N>M`), `CM_`, `VAL_`, `VAL_TABLE_`, `BA_`, `BA_DEF_`, `BA_DEF_DEF_`, `SG_MUL_VAL_` |
| `error` | `DbcError`: the single typed, exhaustive error |

## Motorola bit numbering: done right

DBC files number bits per byte, LSB-first; the byte orders differ in what
`start_bit` names and the direction of travel:

* **Intel (`@1`)**: `start_bit` is the **LSB**; bits ascend linearly.
* **Motorola (`@0`)**: `start_bit` is the **MSB**; extraction descends
  within each byte and jumps from bit 0 of one byte to bit 7 of the next
  (`+15` in DBC numbering: the sawtooth). A `39|16@0+` signal is a
  big-endian `u16` across bytes 4–5; motohawk's `0|12@0-` Temperature
  straddles three bytes.

The coding is cross-verified against [cantools] 44.1.0 on the same embedded
sample files (golden decode *and* encode vectors).

[cantools]: https://github.com/cantools/cantools

## Guarantees

- **Total parser**: `Dbc::parse` never panics: every failure is a typed
  `DbcError::Syntax` with a line number (unknown vendor keywords are
  skipped, not rejected). Deny-level lints
  (`unwrap_used`/`expect_used`/`panic`/`indexing_slicing`) on the lib
  target, a cargo-fuzz target (`dbc_parse`), 30 s CI smoke, miri.
- **Zero-copy decode**: `Signal::decode_raw` borrows the payload; bits
  beyond a short buffer read as zero (documented semantics).
- **Range-checked encode**: `encode_raw` enforces the declared `[min|max]`
  range (when `min < max`), the bit width, and buffer bounds.
- **Multiplexing**: `M`/`m<N>` filter in `Message::unpack`;
  `SG_MUL_VAL_` extended-multiplexing ranges are parsed into
  `Dbc::extended_multiplexing`.

## Example

```rust
use dbc_parse::{ByteOrder, Dbc};

let dbc = Dbc::parse(r#"
BO_ 496 ExampleMessage: 8 PCM1
 SG_ Temperature : 0|12@0- (0.01,250) [229.52|270.47] "degK" PCM1,FOO
 SG_ Enable : 7|1@0+ (1,0) [0|0] "-" Vector__XXX
"#).unwrap();

let values = dbc.messages[0].unpack(&[0x01, 0xA1, 0x02, 0x1A, 0xF8, 0x80, 0x00, 0x00]);
assert_eq!(values[0].0.name, "Temperature");
assert!((values[0].1 - 242.4).abs() < 1e-9); // cantools-verified golden
```

## Attribution

The embedded test files `tests/files/motohawk.dbc` and
`tests/files/multiplex.dbc` come from the [cantools] test corpus (MIT
license); golden vectors were produced with cantools 44.1.0.

## License

MIT OR Apache-2.0: see LICENSE-MIT and LICENSE-APACHE.
