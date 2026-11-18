//! Peak heap usage per pipeline stage, measured with a tracking allocator.
//!
//! `peak` is the highest live heap above the stage's starting level;
//! `retained` is the growth still live when the stage ends. `total` is the
//! highest live heap of the whole run above its starting level.

use clap::Parser;
use safety_profiler::{
    io::{collect_inputs, exclude_inputs, stem, OutDir},
    mem::{self, TrackingAlloc},
    run_pipeline, stage_clean, stage_compile, stage_emit, stage_fold, stage_parse, PipelineError,
    STAGE_NAMES,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

#[global_allocator]
static ALLOC: TrackingAlloc = TrackingAlloc;

/// Profile heap usage of the pipeline stages.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Verilog files and/or directories to profile.
    #[arg(value_name = "PATH", default_value = "inputs")]
    paths: Vec<PathBuf>,

    /// Only circuits whose file stem contains this substring.
    #[arg(short, long)]
    filter: Option<String>,

    /// Skip circuits whose file stem contains this substring (repeatable).
    #[arg(long = "exclude", value_name = "TEXT")]
    excludes: Vec<String>,

    /// Root directory for generated artifacts.
    #[arg(short, long, default_value = "output")]
    out_dir: PathBuf,
}

/// Heap usage of one stage, in bytes.
struct Usage {
    start: usize,
    peak: usize,
    retained: usize,
}

fn measure<T>(f: impl FnOnce() -> Result<T, PipelineError>) -> Result<(T, Usage), String> {
    let start = mem::reset_peak();
    let value = f().map_err(|e| e.to_string())?;
    let usage = Usage {
        start,
        peak: mem::peak().saturating_sub(start),
        retained: mem::current().saturating_sub(start),
    };
    Ok((value, usage))
}

/// Per-stage usage in [`STAGE_NAMES`] order.
fn profile(src: &str, path: &Path) -> Result<Vec<Usage>, String> {
    let (ast, parse) = measure(|| stage_parse(src, path))?;
    let (netlist, compile) = measure(|| stage_compile(&ast))?;
    let (_, clean1) = measure(|| stage_clean(&netlist))?;
    let (_, fold) = measure(|| stage_fold(&netlist))?;
    let (_, clean2) = measure(|| stage_clean(&netlist))?;
    let (_, emit) = measure(|| stage_emit(&netlist))?;
    Ok(vec![parse, compile, clean1, fold, clean2, emit])
}

/// Highest live heap of the run, and heap still live at the end, above the start.
fn totals(usage: &[Usage]) -> (usize, usize) {
    let base = usage[0].start;
    let peak = usage
        .iter()
        .map(|u| u.start - base + u.peak)
        .max()
        .unwrap_or(0);
    let last = &usage[usage.len() - 1];
    (peak, last.start - base + last.retained)
}

fn run(cli: Cli) -> Result<usize, String> {
    let mut inputs = collect_inputs(&cli.paths, cli.filter.as_deref())?;
    exclude_inputs(&mut inputs, &cli.excludes);
    if inputs.is_empty() {
        return Err("no matching .v files remain after exclusions".into());
    }

    let mut header = format!("{:<18}", "circuit");
    for s in STAGE_NAMES {
        let _ = write!(header, " {s:>9}");
    }
    println!(
        "=== Peak heap per stage (KiB) ===\n\n{header} {:>9}",
        "total"
    );
    let mut csv = String::from("circuit,stage,peak_bytes,retained_bytes\n");
    let mut failures = 0;

    for input in &inputs {
        let name = stem(input);
        let result = safety_profiler::io::read_source(input)
            .map_err(|e| format!("read error: {e}"))
            .and_then(|src| {
                run_pipeline(&src, input).map_err(|e| format!("pipeline error: {e}"))?;
                profile(&src, input)
            });
        let usage = match result {
            Ok(u) => u,
            Err(e) => {
                eprintln!("  {name}: {e}");
                failures += 1;
                continue;
            }
        };

        let (peak, retained) = totals(&usage);
        let mut line = format!("{name:<18}");
        for (stage, u) in STAGE_NAMES.iter().zip(&usage) {
            let _ = write!(line, " {:>9.1}", u.peak as f64 / 1024.0);
            let _ = writeln!(csv, "{name},{stage},{},{}", u.peak, u.retained);
        }
        println!("{line} {:>9.1}", peak as f64 / 1024.0);
        let _ = writeln!(csv, "{name},total,{peak},{retained}");
    }

    let dir = OutDir::new(&cli.out_dir)
        .stats()
        .map_err(|e| e.to_string())?;
    let path = dir.join("memory.csv");
    fs::write(&path, csv).map_err(|e| format!("{}: {e}", path.display()))?;
    eprintln!("\nWritten to {}", path.display());
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
