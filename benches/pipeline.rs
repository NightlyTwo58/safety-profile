/// Criterion benchmarks for the safety-profiler pipeline.
///
/// Per-stage isolation works by using the library's public `stage_*` functions.
/// Setup work (everything before the stage under test) runs in the `iter_batched`
/// setup closure, which Criterion does NOT include in the measured time.
///
/// Run all benches:
///     cargo bench
///
/// Single group:
///     cargo bench -- compile
///
/// With per-bench flamegraphs (needs `debug = 1` in [profile.release]):
///     cargo bench --features flamegraph
///     # SVGs: target/criterion/<group>/<circuit>/profile/flamegraph.svg
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use safety_profiler::{stage_clean, stage_compile, stage_emit, stage_fold, stage_parse};
use std::path::PathBuf;

#[cfg(feature = "flamegraph")]
use pprof::criterion::{Output, PProfProfiler};

// ── Corpus ───────────────────────────────────────────────────────────────────
//
// Add entries as you expand the ISCAS-85 corpus.
// Files are embedded at compile time so the bench binary is self-contained.

const INPUTS: &[(&str, &str)] = &[
    ("c17", include_str!("../inputs/c17.v")),
    // ("c432",  include_str!("../inputs/c432.v")),
    // ("c880",  include_str!("../inputs/c880.v")),
    // ("c7552", include_str!("../inputs/c7552.v")),
];

// ── Benchmark groups ─────────────────────────────────────────────────────────

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| stage_parse(src, &path).expect("parse failed"))
        });
    }
    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        let ast = stage_parse(src, &path).unwrap(); // setup, not timed
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter(|| stage_compile(ast).expect("compile failed"))
        });
    }
    group.finish();
}

fn bench_clean1(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean1");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        let ast = stage_parse(src, &path).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || stage_compile(ast).unwrap(),       // fresh netlist each iter (not timed)
                |nl| stage_clean(&nl).expect("clean1 failed"),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_fold(c: &mut Criterion) {
    let mut group = c.benchmark_group("fold");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        let ast = stage_parse(src, &path).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).unwrap();
                    stage_clean(&nl).unwrap();
                    nl
                },
                |nl| stage_fold(&nl).expect("fold failed"),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_clean2(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean2");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        let ast = stage_parse(src, &path).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).unwrap();
                    stage_clean(&nl).unwrap();
                    stage_fold(&nl).unwrap();
                    nl
                },
                |nl| stage_clean(&nl).expect("clean2 failed"),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        let ast = stage_parse(src, &path).unwrap();
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).unwrap();
                    stage_clean(&nl).unwrap();
                    stage_fold(&nl).unwrap();
                    stage_clean(&nl).unwrap();
                    nl
                },
                |nl| stage_emit(&nl).expect("emit failed"),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");
    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{name}.v"));
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| safety_profiler::run_pipeline(src, &path).expect("pipeline failed"))
        });
    }
    group.finish();
}

// ── criterion_group wiring ────────────────────────────────────────────────────

#[cfg(feature = "flamegraph")]
criterion_group! {
    name = pipeline_benches;
    config = Criterion::default()
        .with_profiler(PProfProfiler::new(1000, Output::Flamegraph(None)));
    targets =
        bench_parse, bench_compile,
        bench_clean1, bench_fold, bench_clean2,
        bench_emit, bench_full_pipeline
}

#[cfg(not(feature = "flamegraph"))]
criterion_group!(
    pipeline_benches,
    bench_parse,
    bench_compile,
    bench_clean1,
    bench_fold,
    bench_clean2,
    bench_emit,
    bench_full_pipeline
);

criterion_main!(pipeline_benches);
