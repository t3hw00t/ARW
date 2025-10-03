use arw_core::arrow_ingest::{generate_json, parse_with_arrow, parse_with_serde};
use criterion::{criterion_group, criterion_main, Criterion};

fn bench_ingest(c: &mut Criterion) {
    let data = generate_json(1000);
    c.bench_function("serde_json", |b| b.iter(|| parse_with_serde(&data)));
    c.bench_function("arrow", |b| b.iter(|| parse_with_arrow(&data)));
}

criterion_group!(benches, bench_ingest);
criterion_main!(benches);
