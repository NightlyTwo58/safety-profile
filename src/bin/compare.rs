//! Compare this pipeline against a Yosys baseline on Verilog circuits.
//!
//! Ours is a direct library call. Yosys is a subprocess, so its wall time
//! includes process startup, which is measured once and reported as
//! `x-raw` (with startup) and `x-net` (startup subtracted).

use clap::Parser;
use safety_profiler::{
    flame,
    io::{collect_inputs, exclude_inputs, stem, OutDir},
    report::{Report, Row, YosysRow},
    run_pipeline, run_rename_nets,
    stats::Summary,
    yosys, STAGE_NAMES,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
    time::Duration,
};

/// Benchmark the safety pass pipeline against Yosys.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Verilog files and/or directories to benchmark.
    #[arg(value_name = "PATH", default_value = "inputs")]
    paths: Vec<PathBuf>,

    /// Only run circuits whose file stem contains this substring.
    #[arg(short, long)]
    filter: Option<String>,

    /// Skip circuits whose file stem contains this substring (repeatable).
    #[arg(long = "exclude", value_name = "TEXT")]
    excludes: Vec<String>,

    /// Root directory for generated artifacts.
    #[arg(short, long, default_value = "output")]
    out_dir: PathBuf,

    /// Timed iterations per circuit and tool.
    #[arg(short, long, default_value_t = 20)]
    iters: usize,

    /// Untimed warmup iterations per circuit and tool.
    #[arg(long, default_value_t = 3)]
    warmup: usize,

    /// Add per-stage columns to the table (CSVs always include them).
    #[arg(long)]
    stages: bool,

    /// Print the sequential-rename comparison table (always saved to CSV).
    #[arg(long)]
    rename: bool,

    /// Print the pipeline CSV to stdout instead of the table.
    #[arg(long)]
    csv: bool,

    /// Skip Yosys.
    #[arg(long)]
    no_yosys: bool,

    /// Write a flamegraph per circuit.
    #[arg(long)]
    flamegraph: bool,

    /// Wall-clock budget per flamegraph, in seconds.
    #[arg(long, default_value_t = 2.0)]
    flame_secs: f64,
}

/// Run `f` for the warmup count, then the timed count, and summarize the timed runs.
fn sample(cli: &Cli, mut f: impl FnMut() -> Result<u128, String>) -> Result<Summary, String> {
    for _ in 0..cli.warmup {
        f()?;
    }
    let samples = (0..cli.iters.max(1))
        .map(|_| f())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples))
}

/// Per-stage and total summaries for our pipeline, plus the last emitted Verilog.
fn measure_ours(
    cli: &Cli,
    src: &str,
    path: &Path,
) -> Result<(Vec<Summary>, Summary, String), String> {
    let run = || run_pipeline(src, path).map_err(|e| format!("pipeline error: {e}"));
    for _ in 0..cli.warmup {
        run()?;
    }
    let iters = cli.iters.max(1);
    let mut per_stage = vec![Vec::with_capacity(iters); STAGE_NAMES.len()];
    let mut totals = Vec::with_capacity(iters);
    let mut verilog = String::new();
    for _ in 0..iters {
        let result = run()?;
        for (samples, name) in per_stage.iter_mut().zip(STAGE_NAMES) {
            samples.push(result.stage(name).micros());
        }
        totals.push(result.total_micros());
        verilog = result.verilog;
    }
    let stages = per_stage.iter().map(|s| Summary::from_micros(s)).collect();
    Ok((stages, Summary::from_micros(&totals), verilog))
}

fn measure_yosys(cli: &Cli, out: &OutDir, input: &Path, name: &str) -> Result<YosysRow, String> {
    let dir = out.verilog("yosys").map_err(|e| e.to_string())?;
    let script = yosys::pipeline_script(input, &dir.join(format!("{name}.v")));
    let pipeline = sample(cli, || yosys::run(&script)).map_err(|e| format!("yosys error: {e}"))?;

    let rename_rounds = yosys::rename_rounds(input)?;
    let rename = sample(cli, || yosys::rename_micros(input, rename_rounds))
        .map_err(|e| format!("yosys rename error: {e}"))?;
    Ok(YosysRow {
        pipeline,
        rename,
        rename_rounds,
    })
}

/// Measure one circuit with our pipeline and, unless disabled, Yosys.
fn bench(cli: &Cli, out: &OutDir, input: &Path, src: &str, name: &str) -> Result<Row, String> {
    let (stages, ours, verilog) = measure_ours(cli, src, input)?;
    let saved = out
        .verilog("ours")
        .map_err(|e| e.to_string())
        .and_then(|dir| {
            fs::write(dir.join(format!("{name}.v")), &verilog).map_err(|e| e.to_string())
        });
    if let Err(e) = saved {
        eprintln!("  {name}: could not save our output: {e}");
    }

    let rename_ours = sample(cli, || {
        run_rename_nets(src, input)
            .map(|stage| stage.micros())
            .map_err(|e| format!("rename nets error: {e}"))
    })?;

    let baseline = if cli.no_yosys {
        None
    } else {
        Some(measure_yosys(cli, out, input, name)?)
    };
    Ok(Row {
        name: name.to_string(),
        stages,
        ours,
        rename_ours,
        yosys: baseline,
    })
}

fn write_flamegraph(
    cli: &Cli,
    out: &OutDir,
    input: &Path,
    src: &str,
    name: &str,
) -> Result<(), String> {
    let svg = out.pipeline_flamegraph(name).map_err(|e| e.to_string())?;
    let budget = Duration::from_secs_f64(cli.flame_secs);
    let iters = flame::profile_pipeline(src, input, budget, &svg)?;
    eprintln!(
        "  {name}: flamegraph ({iters} iterations) -> {}",
        svg.display()
    );
    Ok(())
}

/// Run every selected circuit; returns the number that failed.
fn run(cli: Cli) -> Result<usize, String> {
    let mut inputs = collect_inputs(&cli.paths, cli.filter.as_deref())?;
    let excluded = exclude_inputs(&mut inputs, &cli.excludes);
    if excluded > 0 {
        eprintln!("Excluded {excluded} input(s) by --exclude.");
    }
    if inputs.is_empty() {
        return Err("no matching .v files remain after exclusions".into());
    }

    let out = OutDir::new(&cli.out_dir);
    let startup = if cli.no_yosys {
        None
    } else {
        Some(yosys::startup(cli.iters)?)
    };
    let mut report = Report::new(startup, cli.stages);
    if cli.csv {
        print!("{}", report.summary_csv_header());
    } else {
        print!("{}", report.table_header());
    }

    let mut failures = 0;
    for input in &inputs {
        let name = stem(input);
        let measured = safety_profiler::io::read_source(input)
            .map_err(|e| format!("read error: {e}"))
            .and_then(|src| bench(&cli, &out, input, &src, &name).map(|row| (src, row)));
        let (src, row) = match measured {
            Ok(v) => v,
            Err(e) => {
                eprintln!("  {name}: {e}");
                failures += 1;
                continue;
            }
        };

        if cli.csv {
            print!("{}", report.summary_csv_row(&row));
        } else {
            print!("{}", report.table_row(&row));
        }
        report.push(row);

        let flame = cli
            .flamegraph
            .then(|| write_flamegraph(&cli, &out, input, &src, &name));
        if let Some(Err(e)) = flame {
            eprintln!("  {name}: flamegraph failed: {e}");
        }
    }

    if report.is_empty() {
        eprintln!("\nNo successful comparisons; no stats files written.");
    } else {
        if cli.rename && !cli.csv {
            print!("{}", report.rename_table());
        }
        let dir = out.stats().map_err(|e| e.to_string())?;
        report.write_csvs(&dir)?;
        eprintln!("\nStats written to {}", dir.display());
    }
    if let Some(startup) = report.startup() {
        eprintln!("Yosys startup baseline: {startup:.0}µs (x-net subtracts this)");
    }
    Ok(failures)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(0) => ExitCode::SUCCESS,
        Ok(n) => {
            eprintln!("{n} circuit(s) failed.");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
