//! Criterion per-stage benchmarks.
//!
//! Each stage runs on a fresh netlist prepared by the preceding stages in the
//! `iter_batched` setup closure, which Criterion excludes from the timing.
//! Every `.v` file in `inputs/` is benchmarked.
//!
//! ```sh
//! cargo bench                          # all groups
//! cargo bench -- compile               # one group
//! cargo bench --features flamegraph    # SVGs in target/criterion/<group>/<circuit>/profile/
//! ```

use criterion::{BatchSize, BenchmarkId, Criterion, criterion_group, criterion_main};
use safety_profiler::{
    Net, io::{collect_inputs, stem}, run_pipeline, stage_clean, stage_compile, stage_emit,
    stage_fold, stage_parse, stage_rename_nets,
};
use std::{fs, path::PathBuf};

#[cfg(feature = "flamegraph")]
use pprof::criterion::{Output, PProfProfiler};

/// A benchmark circuit: name, source text, and path.
type Circuit = (String, String, PathBuf);

/// Load every input circuit under `inputs/`, sorted by path.
fn load_inputs() -> Vec<Circuit> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inputs");
    collect_inputs(&[dir], None)
        .unwrap_or_else(|e| panic!("{e}"))
        .into_iter()
        .map(|path| {
            let src = fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("failed to read {}: {e}", path.display()));
            (stem(&path), src, path)
        })
        .collect()
}

fn after_clean(nl: &Net) {
    stage_clean(nl).expect("clean1 failed");
}

fn after_fold(nl: &Net) {
    after_clean(nl);
    stage_fold(nl).expect("fold failed");
}

fn after_optimize(nl: &Net) {
    after_fold(nl);
    stage_clean(nl).expect("clean2 failed");
}

/// Benchmark `run` on a netlist that `prep` has already advanced.
fn bench_stage<R>(c: &mut Criterion, group: &str, prep: fn(&Net), run: fn(&Net) -> R) {
    let mut group = c.benchmark_group(group);
    for (name, src, path) in &load_inputs() {
        let ast = stage_parse(src, path).expect("parse failed");
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter_batched(
                || {
                    let nl = stage_compile(ast).expect("compile failed");
                    prep(&nl);
                    nl
                },
                |nl| run(&nl),
                BatchSize::SmallInput,
            )
        });
    }
    group.finish();
}

fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");
    for (name, src, path) in &load_inputs() {
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| stage_parse(src, path).expect("parse failed"))
        });
    }
    group.finish();
}

fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");
    for (name, src, path) in &load_inputs() {
        let ast = stage_parse(src, path).expect("parse failed");
        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter(|| stage_compile(ast).expect("compile failed"))
        });
    }
    group.finish();
}

fn bench_passes(c: &mut Criterion) {
    bench_stage(c, "clean1", |_| {}, |nl| stage_clean(nl).expect("clean1 failed"));
    bench_stage(c, "fold", after_clean, |nl| stage_fold(nl).expect("fold failed"));
    bench_stage(c, "clean2", after_fold, |nl| stage_clean(nl).expect("clean2 failed"));
    bench_stage(c, "emit", after_optimize, |nl| stage_emit(nl).expect("emit failed"));
    bench_stage(c, "rename_nets", |_| {}, |nl| {
        stage_rename_nets(nl).expect("rename nets failed")
    });
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");
    for (name, src, path) in &load_inputs() {
        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| run_pipeline(src, path).expect("pipeline failed"))
        });
    }
    group.finish();
}

#[cfg(feature = "flamegraph")]
criterion_group! {
    name = pipeline_benches;
    config = Criterion::default()
        .measurement_time(std::time::Duration::from_secs(10))
        .with_profiler(PProfProfiler::new(1000, Output::Flamegraph(None)));
    targets = bench_parse, bench_compile, bench_passes, bench_full_pipeline
}

#[cfg(not(feature = "flamegraph"))]
criterion_group!(
    pipeline_benches,
    bench_parse,
    bench_compile,
    bench_passes,
    bench_full_pipeline
);

criterion_main!(pipeline_benches);
