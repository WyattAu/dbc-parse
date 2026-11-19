# PATTERN_ADOPTION

Relationship of dbc-parse to the WyattAu pattern canon
(WyattAu/pattern-standards, `references/catalog.md`). Statuses and row
format follow `references/adoption-format.md`. dbc-parse is an L0 leaf
library: a `no_std` Vector DBC parser with zero runtime dependencies and
no I/O of any kind.

| Pattern | Status | Evidence |
|---|---|---|
| T1 | adopted | src/model.rs:439 (`Dbc::parse` turns boundary input into the typed `Dbc` model once; interior code only sees constructed values, and every failure is the exhaustive `DbcError` in src/error.rs) |
| T13 | adopted | src/lib.rs:69 (the whole crate is the pure core: `no_std`, zero runtime dependencies, no I/O; hosts own the shell, and the test suites assert on plain inputs with no mocks, dev-deps are proptest and criterion per Cargo.toml:27) |
| P64 | adopted | tests/properties.rs:105 (4 proptest invariants at 500 cases: Intel and Motorola encode-decode round-trips, decode totality over arbitrary buffers, parse totality over arbitrary printable text at tests/properties.rs:136) |
| P65 | adopted | tests/parse.rs:9 (motohawk and multiplex sample files under tests/files/ asserted against golden decode and encode vectors cross-verified with cantools 44.1.0, the independent reference implementation; vectors come from the reference, not from snapshotting this crate's own output) |
| P67 | partial | fuzz/fuzz_targets/dbc_parse.rs:9 (libFuzzer target: parse totality plus decode of every parsed message over an arbitrary payload), fuzz/corpus/dbc_parse/ (checked-in corpus), .github/workflows/ci.yml:31 (30 s smoke on push, PR, and weekly cron); the nightly enlarged-case soak (`PROPTEST_CASES=4096`) required by the adoption format is not yet scheduled, target 2027-Q1 |
| P68 | adopted | tests/properties.rs:35 (`build_signal` and `round_trip_cases` strategies build signals from generated scale, offset, and bit fields; no shared mutable fixtures) |
| P70 | adopted | src/parser.rs:180 (unknown keywords are skipped line-wise via `skip_to_semicolon`, so vendor additions parse; BA_DEF_ schemas are captured or skipped per tests/parse.rs:57) |
| P37 | n/a | one-shot deterministic parse with no upstream retry loop to repair ahead of; tolerance-by-skip is declared under P70, and malformed known constructs yield a typed `DbcError::Syntax` rather than a minimal edit |
| P116 | n/a | library crate with no upload or file-ingestion surface; hostile-input defense is deny-level panic lints at Cargo.toml:46 and the P67 fuzz target, which are parser-bound mechanisms, not upload hardening |
