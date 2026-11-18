//! Run the pipeline on one Verilog file and print per-stage timings.

use clap::Parser;
use safety_profiler::{
    flame,
    io::{stem, OutDir},
    run_pipeline, run_rename_nets,
    stats::Summary,
};
use std::{fs, path::PathBuf, process::ExitCode, time::Duration};

/// Run the safety pass pipeline on one Verilog file and print stage timings.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Input Verilog file.
    input: PathBuf,

    /// Root directory for generated artifacts.
    #[arg(short, long, default_value = "output")]
    out_dir: PathBuf,

    /// Number of runs; with more than one, median/min/stddev are reported.
    #[arg(short, long, default_value_t = 1)]
    iters: usize,

    /// Write a pipeline flamegraph under <out-dir>/flamegraphs/.
    #[arg(long)]
    flamegraph: bool,

    /// Write the emitted Verilog under <out-dir>/verilog/ours/.
    #[arg(long)]
    save_verilog: bool,

    /// Wall-clock budget for the flamegraph run, in seconds.
    #[arg(long, default_value_t = 2.0)]
    flame_secs: f64,
}

/// Print one stage line: a single time for one sample, else median/min/sd.
fn print_summary(name: &str, samples: &[u128], note: &str) {
    let summary = Summary::from_micros(samples);
    if samples.len() == 1 {
        println!("[{name}]  {:.0}µs  {note}", summary.median);
    } else {
        println!(
            "[{name}]  median {:.1}µs  min {:.1}  sd {:.1}  {note}",
            summary.median, summary.min, summary.stddev
        );
    }
}

fn run(cli: Cli) -> Result<(), String> {
    let out = OutDir::new(&cli.out_dir);
    let src =
        fs::read_to_string(&cli.input).map_err(|e| format!("{}: {e}", cli.input.display()))?;
    let name = stem(&cli.input);
    let iters = cli.iters.max(1);

    let runs = (0..iters)
        .map(|_| run_pipeline(&src, &cli.input).map_err(|e| format!("pipeline failed: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let renames = (0..iters)
        .map(|_| run_rename_nets(&src, &cli.input).map_err(|e| format!("rename nets failed: {e}")))
        .collect::<Result<Vec<_>, _>>()?;
    let last = &runs[iters - 1];

    for (i, stage) in last.stages.iter().enumerate() {
        let samples: Vec<u128> = runs.iter().map(|r| r.stages[i].micros()).collect();
        print_summary(stage.name, &samples, stage.note.as_deref().unwrap_or(""));
    }
    let samples: Vec<u128> = renames.iter().map(|s| s.micros()).collect();
    let note = renames[iters - 1].note.as_deref().unwrap_or("");
    print_summary("rename_nets", &samples, note);

    if cli.save_verilog {
        let path = out
            .verilog("ours")
            .map_err(|e| e.to_string())?
            .join(format!("{name}.v"));
        fs::write(&path, &last.verilog).map_err(|e| format!("{}: {e}", path.display()))?;
        eprintln!("Verilog written to {}", path.display());
    }

    if cli.flamegraph {
        let svg = out.pipeline_flamegraph(&name).map_err(|e| e.to_string())?;
        let budget = Duration::from_secs_f64(cli.flame_secs);
        let n = flame::profile_pipeline(&src, &cli.input, budget, &svg)?;
        eprintln!("Flamegraph ({n} iterations): {}", svg.display());
    }
    Ok(())
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
