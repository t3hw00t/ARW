use axum::http::HeaderMap;
use criterion::{criterion_group, criterion_main, Criterion};
use std::hint::black_box;

const CURRENT: &str = "X-ARW-Capsule";
const LEGACY: &str = concat!("X-ARW", "-Gate");

fn borrowed_header<'a>(headers: &'a HeaderMap, name: &'a str) -> Option<&'a str> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn cloned_header(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

fn bench_capsule_headers(c: &mut Criterion) {
    let mut current_headers = HeaderMap::new();
    current_headers.insert(CURRENT, " {\"id\":\"demo\"} ".parse().unwrap());

    let mut legacy_headers = HeaderMap::new();
    legacy_headers.insert(LEGACY, " {\"id\":\"legacy\"} ".parse().unwrap());

    c.bench_function("capsule_header_borrowed_current", |b| {
        b.iter(|| black_box(borrowed_header(&current_headers, CURRENT)))
    });

    c.bench_function("capsule_header_borrowed_legacy", |b| {
        b.iter(|| black_box(borrowed_header(&legacy_headers, LEGACY)))
    });

    c.bench_function("capsule_header_cloned_current", |b| {
        b.iter(|| black_box(cloned_header(&current_headers, CURRENT)))
    });

    c.bench_function("capsule_header_cloned_legacy", |b| {
        b.iter(|| black_box(cloned_header(&legacy_headers, LEGACY)))
    });
}

criterion_group!(benches, bench_capsule_headers);
criterion_main!(benches);
