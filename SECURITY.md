# Security Policy — dbc-parse

## Supported versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | ✅        |

## Reporting a vulnerability

Report privately via [GitHub security advisories] for this repository, or
email **wyatt_au@protonmail.com**. Do **not** open a public issue for
security reports.

You will receive an acknowledgement within **72 hours**. Coordinated
disclosure: we ask for up to 90 days before public disclosure while a
patch ships.

[GitHub security advisories]: https://github.com/WyattAu/dbc-parse/security/advisories/new

## Threat model

**I/O surface: untrusted input.** DBC files come from Vector tools, OEM
exports, and random internet repositories — hostile text is the norm. The
trust boundary is the *contents* of every `&str` handed to `Dbc::parse`
and every `&[u8]` handed to `Signal::decode_raw` / `Message::unpack`.
Callers control where the bytes come from; dbc-parse guarantees safety of
the *parse and decode* regardless.

| ID | Threat | Mitigation |
|----|--------|------------|
| T1 | Malformed/hostile DBC text drives the parser out of bounds or into a panic | Totality by construction: a hand-rolled bounds-checked scanner (`get()`-only access; `unwrap`/`expect`/`panic`/`indexing_slicing` denied at the lib target), exhaustive `DbcError`, cargo-fuzz target `dbc_parse`, 30 s CI smoke, miri |
| T2 | Adversarial layouts make bit extraction read/write out of bounds | `decode_raw` reads zero for missing bytes; `encode_raw` pre-computes the touched byte range and rejects short buffers (`BufferTooShort`) — never an out-of-bounds write |
| T3 | Non-finite or oversized physical values poison downstream control logic | `encode_raw` rejects NaN/infinities and declared-range violations (`ValueOutOfRange`), and raws that do not fit the signal width (`RawOutOfRange`) |
| T4 | Malicious dependency substitution in the build chain | cargo-deny (advisories/licenses/bans) and cargo-vet gates in CI; supply-chain audits committed; the crate has **zero** runtime dependencies |

## Scope

`dbc-parse` is a parser and coding library: it performs no I/O, spawns no
threads, and never executes attacker-controlled code. Memory-safety bugs
would require an `unsafe` block — the crate contains none
(`unsafe_code = "deny"`).
