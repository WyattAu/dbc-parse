# Changelog

All notable changes to this project will be documented in this file.
Format: [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versioning:
[SemVer](https://semver.org/).

## [0.1.0] — 2026-10-03

Initial release — L0 leaf, zero estate deps, zero runtime deps, `no_std` +
`alloc`.

### Added

- **Parser**: `Dbc::parse` over the Vector DBC grammar — `VERSION`, `NS_`,
  `BS_`, `BU_`, `BO_` (extended ids via bit 31), `SG_` with `M`/`m<N>`/
  `m<N>M` multiplexing, `CM_` (multi-line `""`-escaped strings), `VAL_`,
  `VAL_TABLE_`, `BA_`, and `SG_MUL_VAL_` extended multiplexing; unknown
  vendor keywords are skipped, malformed known constructs are typed
  `DbcError::Syntax` errors with line numbers.
- **Model**: `Dbc`, `Message` (`unpack` with multiplexor filtering),
  `Signal` (`decode_raw`/`encode_raw` with scale/offset, sign extension,
  range/width/buffer checks), `Node`, `ValueTable`, `Attribute`,
  `ExtendedMux`, plus a self-contained range-validated `CanId` (L0 crates
  carry no estate deps, so no `can-core` dependency).
- **Bit coding**: Motorola (big-endian, sawtooth) and Intel
  (little-endian, linear) extraction/insertion — cross-verified against
  cantools 44.1.0 golden vectors on embedded real-world DBC files.
- **Gates**: 500-case proptest battery (Intel + Motorola encode/decode
  round-trips, decode/parse totality), cargo-fuzz target `dbc_parse` with
  committed corpus, miri, embedded (`thumbv7em-none-eabihf`), tier-a
  shared CI pinned to engineering-standards `main`.

[0.1.0]: https://github.com/WyattAu/dbc-parse/releases/tag/v0.1.0
