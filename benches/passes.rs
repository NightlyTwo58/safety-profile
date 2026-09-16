// benches/pipeline.rs
use criterion::{criterion_group, criterion_main, Criterion};
use pprof::criterion::{O    utput, PProfProfiler};

fn bench_parse(c: &mut Criterion) {
    let src = include_str!("../inputs/c17.v");
    c.bench_function("parse c17", |b| {
        b.iter(|| parse_sv_str(src, ...))
    });
}

criterion_group! {
    name = benches;
    config = Criterion::default().with_profiler(PProfProfiler::new(100, Output::Flamegraph(None)));
    targets = bench_parse, bench_compile, bench_clean, bench_fold
}
criterion_main!(benches);