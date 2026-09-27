/// Criterion benchmarks for the safety-profiler pipeline.
///
/// Per-stage isolation works by using the library's public `stage_*` functions.
/// Setup work (everything before the stage under test) runs in the `iter_batched`
/// setup closure, which Criterion does NOT include in the measured time.
///
/// Every `.v` file in the `inputs/` directory is automatically included.
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
use std::fs;
use std::path::PathBuf;

#[cfg(feature = "flamegraph")]
use pprof::criterion::{Output, PProfProfiler};

// Corpus
//
// Automatically load every .v file from the project's inputs/ directory.
// Files are sorted by filename so benchmark ordering is deterministic.

type Input = (String, String, PathBuf);

fn load_inputs() -> Vec<Input> {
    let inputs_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inputs");

    let mut inputs: Vec<Input> = fs::read_dir(&inputs_dir)
        .unwrap_or_else(|e| {
            panic!(
                "failed to read inputs directory {}: {e}",
                inputs_dir.display()
            )
        })
        .filter_map(|entry| {
            let entry = entry.expect("failed to read directory entry");
            let path = entry.path();

            if path.extension().and_then(|ext| ext.to_str()) != Some("v") {
                return None;
            }

            let name = path.file_stem()?.to_str()?.to_owned();
            let src = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));

            Some((name, src, path))
        })
        .collect();

    inputs.sort_by(|a, b| a.0.cmp(&b.0));

    assert!(
        !inputs.is_empty(),
        "no .v files found in {}",
        inputs_dir.display()
    );

    inputs
}

// Benchmark groups

fn bench_parse(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut group = c.benchmark_group("parse");

    for (name, src, path) in &inputs {
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| stage_parse(src, path).expect("parse failed"))
        });
    }

    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut group = c.benchmark_group("compile");

    for (name, src, path) in &inputs {
        let ast = stage_parse(src, path).expect("parse failed"); // setup, not timed

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter(|| stage_compile(ast).expect("compile failed"))
        });
    }

    group.finish();
}

fn bench_clean1(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut group = c.benchmark_group("clean1");

    for (name, src, path) in &inputs {
        let ast = stage_parse(src, path).expect("parse failed");

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || stage_compile(ast).expect("compile failed"), // setup, not timed
                |nl| stage_clean(&nl).expect("clean1 failed"),
                BatchSize::SmallInput,
            )
        });
    }

    group.finish();
}

fn bench_fold(c: &mut Criterion) {
    let inputs = load_inputs();
    let mut group = c.benchmark_group("fold");

    for (name, src, path) in &inputs {
        let ast = stage_parse(src, path).expect("parse failed");

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).expect("compile failed");
                    stage_clean(&nl).expect("clean1 failed");
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
    let inputs = load_inputs();
    let mut group = c.benchmark_group("clean2");

    for (name, src, path) in &inputs {
        let ast = stage_parse(src, path).expect("parse failed");

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).expect("compile failed");
                    stage_clean(&nl).expect("clean1 failed");
                    stage_fold(&nl).expect("fold failed");
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
    let inputs = load_inputs();
    let mut group = c.benchmark_group("emit");

    for (name, src, path) in &inputs {
        let ast = stage_parse(src, path).expect("parse failed");

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).expect("compile failed");
                    stage_clean(&nl).expect("clean1 failed");
                    stage_fold(&nl).expect("fold failed");
                    stage_clean(&nl).expect("clean2 failed");
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
    let inputs = load_inputs();
    let mut group = c.benchmark_group("full_pipeline");

    for (name, src, path) in &inputs {
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| {
                safety_profiler::run_pipeline(src, path).expect("pipeline failed")
            })
        });
    }

    group.finish();
}

// Criterion group wiring

#[cfg(feature = "flamegraph")]
criterion_group! {
    name = pipeline_benches;
    config = Criterion::default()
        .measurement_time(std::time::Duration::from_secs(10))
        .with_profiler(PProfProfiler::new(1000, Output::Flamegraph(None)));
    targets =
        bench_parse,
        bench_compile,
        bench_clean1,
        bench_fold,
        bench_clean2,
        bench_emit,
        bench_full_pipeline
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