//! Criterion benchmark for `HostRouter::lookup` (issue #34).
//!
//! Answers the FAQ "does vhost routing cost me anything?". The lookup is a
//! single `HashMap` probe on a normalized hostname, so both the hit and the
//! miss (fallthrough) paths should stay flat as the number of registered
//! vhosts grows — O(1) in practice.
//!
//! Run with: `cargo bench -p lithair-core --bench host_router`

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use lithair_core::http::host_router::HostRouter;
use std::hint::black_box;

/// Vhost table sizes from the issue: 1, 10, 100, 1000.
const SIZES: &[usize] = &[1, 10, 100, 1000];

fn build_router(n: usize) -> HostRouter<usize> {
    let mut router = HostRouter::new();
    for i in 0..n {
        router.insert(format!("host-{i}.test"), i);
    }
    router
}

fn bench_lookup(c: &mut Criterion) {
    let mut group = c.benchmark_group("host_router_lookup");
    for &n in SIZES {
        let router = build_router(n);

        // Exact match, input already in normalized form.
        let exact = format!("host-{}.test", n - 1);
        group.bench_with_input(BenchmarkId::new("hit_exact", n), &exact, |b, host| {
            b.iter(|| black_box(&router).lookup(black_box(host)))
        });

        // Realistic wire form: mixed case + explicit port, exercising
        // the normalization path (lowercase + port strip) before the probe.
        let wire = format!("HOST-{}.TEST:443", n - 1);
        group.bench_with_input(BenchmarkId::new("hit_wire_form", n), &wire, |b, host| {
            b.iter(|| black_box(&router).lookup(black_box(host)))
        });

        // Unknown host with no default: the full fallthrough path.
        group.bench_with_input(BenchmarkId::new("miss", n), &n, |b, _| {
            b.iter(|| black_box(&router).lookup(black_box("unknown.example")))
        });
    }
    group.finish();
}

criterion_group!(benches, bench_lookup);
criterion_main!(benches);
