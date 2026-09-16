/// Criterion benchmarks for the safety-profiler pipeline.
///
/// Each benchmark group isolates one stage by doing all preceding work in the
/// setup closure (which Criterion does NOT measure) and only timing the stage
/// itself in the routine closure.
///
/// Run all benches:
///     cargo bench
///
/// Run a single group:
///     cargo bench -- parse
///
/// Generate flamegraphs (requires the pprof feature in Cargo.toml):
///     cargo bench -- --profile-time 5
///     # SVGs land in target/criterion/<bench>/profile/flamegraph.svg
use criterion::{criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion};
use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog};
use safety_pass::{Cell, Pass};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::PathBuf;
use sv_parser::parse_sv_str;

#[cfg(feature = "flamegraph")]
use pprof::criterion::{Output, PProfProfiler};

//  Fixture loading 

/// All circuits we benchmark against.
/// Add entries here as you expand the ISCAS-85 corpus.
const INPUTS: &[(&str, &str)] = &[
    ("c17", include_str!("../inputs/c17.v")),
    // ("c432",  include_str!("../inputs/c432.v")),
    // ("c880",  include_str!("../inputs/c880.v")),
    // ("c7552", include_str!("../inputs/c7552.v")),
];

/// Parse a source string and return the AST.
/// All other setup functions call this; it is NOT counted in their timings.
fn do_parse(src: &str, name: &str) -> sv_parser::SyntaxTree {
    let path = PathBuf::from(format!("inputs/{}.v", name));
    let (ast, _) = parse_sv_str(src, path, &HashMap::new(), &[] as &[PathBuf], true, false)
        .unwrap_or_else(|e| panic!("parse failed for {}: {}", name, e));
    ast
}

/// Compile an AST to a netlist. Returns the first (top) module.
fn do_compile(ast: &sv_parser::SyntaxTree) -> std::rc::Rc<safety_pass::Netlist<Cell>> {
    from_vast::<Cell>(ast)
        .expect("compile failed")
        .into_iter()
        .next()
        .expect("no modules in file")
}

//  Benchmark groups 

/// Stage 1 — parse only.
fn bench_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("parse");

    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{}.v", name));

        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| {
                parse_sv_str(src, path.clone(), &HashMap::new(), &[] as &[PathBuf], true, false)
                    .expect("parse failed")
            })
        });
    }

    group.finish();
}

/// Stage 2 — compile (AST → Netlist).
fn bench_compile(c: &mut Criterion) {
    let mut group = c.benchmark_group("compile");

    for (name, src) in INPUTS {
        // Parse once; this cost is NOT counted.
        let ast = do_parse(src, name);

        group.bench_with_input(BenchmarkId::from_parameter(name), &ast, |b, ast| {
            b.iter(|| from_vast::<Cell>(ast).expect("compile failed"))
        });
    }

    group.finish();
}

/// Stage 3 — Clean pass 1.
/// Setup: parse + compile. Only Clean is timed.
/// Because Clean mutates the netlist, we rebuild it fresh per iteration.
fn bench_clean1(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean1");

    for (name, src) in INPUTS {
        let ast = do_parse(src, name);

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &ast,
            |b, ast| {
                b.iter_batched(
                    || do_compile(ast),         // setup (not timed)
                    |netlist| {                 // routine (timed)
                        Clean(PhantomData::<Cell>)
                            .run(&netlist)
                            .expect("clean1 failed")
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

/// Stage 4 — FoldAllPatterns.
/// Setup: parse + compile + clean1.
fn bench_fold(c: &mut Criterion) {
    let mut group = c.benchmark_group("fold");

    for (name, src) in INPUTS {
        let ast = do_parse(src, name);

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &ast,
            |b, ast| {
                b.iter_batched(
                    || {
                        let nl = do_compile(ast);
                        Clean(PhantomData::<Cell>).run(&nl).unwrap();
                        nl
                    },
                    |netlist| FoldAllPatterns.run(&netlist).expect("fold failed"),
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

/// Stage 5 — Clean pass 2.
/// Setup: parse + compile + clean1 + fold.
fn bench_clean2(c: &mut Criterion) {
    let mut group = c.benchmark_group("clean2");

    for (name, src) in INPUTS {
        let ast = do_parse(src, name);

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &ast,
            |b, ast| {
                b.iter_batched(
                    || {
                        let nl = do_compile(ast);
                        Clean(PhantomData::<Cell>).run(&nl).unwrap();
                        FoldAllPatterns.run(&nl).unwrap();
                        nl
                    },
                    |netlist| {
                        Clean(PhantomData::<Cell>)
                            .run(&netlist)
                            .expect("clean2 failed")
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

/// Stage 6 — Emit (PrintVerilog).
/// Setup: full pipeline minus emit.
fn bench_emit(c: &mut Criterion) {
    let mut group = c.benchmark_group("emit");

    for (name, src) in INPUTS {
        let ast = do_parse(src, name);

        group.bench_with_input(
            BenchmarkId::from_parameter(name),
            &ast,
            |b, ast| {
                b.iter_batched(
                    || {
                        let nl = do_compile(ast);
                        Clean(PhantomData::<Cell>).run(&nl).unwrap();
                        FoldAllPatterns.run(&nl).unwrap();
                        Clean(PhantomData::<Cell>).run(&nl).unwrap();
                        nl
                    },
                    |netlist| {
                        PrintVerilog(PhantomData::<Cell>)
                            .run(&netlist)
                            .expect("emit failed")
                    },
                    BatchSize::SmallInput,
                )
            },
        );
    }

    group.finish();
}

/// Full end-to-end pipeline in a single bench.
/// Useful for catching overall regressions when per-stage attribution isn't needed.
fn bench_full_pipeline(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_pipeline");

    for (name, src) in INPUTS {
        let path = PathBuf::from(format!("inputs/{}.v", name));

        group.bench_with_input(BenchmarkId::from_parameter(name), src, |b, src| {
            b.iter(|| {
                let (ast, _) = parse_sv_str(
                    src,
                    path.clone(),
                    &HashMap::new(),
                    &[] as &[PathBuf],
                    true,
                    false,
                )
                    .unwrap();
                let nl = from_vast::<Cell>(&ast).unwrap().into_iter().next().unwrap();
                Clean(PhantomData::<Cell>).run(&nl).unwrap();
                FoldAllPatterns.run(&nl).unwrap();
                Clean(PhantomData::<Cell>).run(&nl).unwrap();
                PrintVerilog(PhantomData::<Cell>).run(&nl).unwrap()
            })
        });
    }

    group.finish();
}

//  criterion_group wiring 
//
// Two variants: one with pprof flamegraphs (--features flamegraph), one without.
// This avoids making pprof a mandatory dependency for a plain `cargo bench`.

#[cfg(feature = "flamegraph")]
criterion_group! {
    name = pipeline_benches;
    config = Criterion::default()
        .with_profiler(PProfProfiler::new(
            1000,                             // sampling freq (Hz)
            Output::Flamegraph(None),         // writes flamegraph.svg
        ));
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