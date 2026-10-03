//! Criterion bench: DBC parse and signal decode hot paths.

// Test/bench file: criterion_group! generates undocumented items, and bench
// bodies unwrap known-good fixtures by design.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing
)]

use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

const MOTOHAWK: &str = include_str!("../tests/files/motohawk.dbc");
const MULTIPLEX: &str = include_str!("../tests/files/multiplex.dbc");

fn bench_parse(c: &mut Criterion) {
    c.bench_function("parse_motohawk", |b| {
        b.iter(|| Dbc::parse(black_box(MOTOHAWK)).unwrap());
    });
    c.bench_function("parse_multiplex", |b| {
        b.iter(|| Dbc::parse(black_box(MULTIPLEX)).unwrap());
    });
}

fn bench_decode(c: &mut Criterion) {
    let dbc = Dbc::parse(MOTOHAWK).unwrap();
    let message = &dbc.messages[0];
    let payload = [0x01, 0xA1, 0x02, 0x1A, 0xF8, 0x80, 0x00, 0x00];
    c.bench_function("unpack_motohawk", |b| {
        b.iter(|| message.unpack(black_box(&payload)));
    });
}

use dbc_parse::Dbc;

criterion_group!(benches, bench_parse, bench_decode);
criterion_main!(benches);
