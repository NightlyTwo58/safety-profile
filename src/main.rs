use clap::Parser;
use safety_profiler::{flame, io::OutDir, run_pipeline, stats::Summary, PipelineResult};
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

    /// Also write a flamegraph to <out-dir>/flamegraphs/.
    #[arg(long)]
    flamegraph: bool,

    /// Wall-clock budget for the flamegraph run, in seconds.
    #[arg(long, default_value_t = 2.0)]
    flame_secs: f64,
}

fn run(cli: Cli) -> Result<(), String> {
    let out = OutDir::new(&cli.out_dir);
    // File I/O is outside all timers: we measure pass performance, not disk.
    let src = fs::read_to_string(&cli.input)
        .map_err(|e| format!("{}: {e}", cli.input.display()))?;
    let stem = cli.input.file_stem().unwrap_or_default().to_string_lossy().into_owned();

    let runs: Vec<PipelineResult> = (0..cli.iters.max(1))
        .map(|_| run_pipeline(&src, &cli.input).map_err(|e| format!("pipeline failed: {e}")))
        .collect::<Result<_, _>>()?;
    let last = runs.last().expect("at least one run");

    for (i, stage) in last.stages.iter().enumerate() {
        let samples: Vec<u128> = runs.iter().map(|r| r.stages[i].micros()).collect();
        let s = Summary::from_micros(&samples);
        let note = stage.note.as_deref().unwrap_or("");
        if runs.len() == 1 {
            println!("[{}]  {:.0}µs  {note}", stage.name, s.median);
        } else {
            println!(
                "[{}]  median {:.1}µs  min {:.1}  sd {:.1}  {note}",
                stage.name, s.median, s.min, s.stddev
            );
        }
    }

    let v_dir = out.verilog("ours").map_err(|e| e.to_string())?;
    let v_path = v_dir.join(format!("{stem}.v"));
    fs::write(&v_path, &last.verilog).map_err(|e| format!("{}: {e}", v_path.display()))?;
    eprintln!("Written to {}", v_path.display());

    if cli.flamegraph {
        let svg = out.flamegraphs().map_err(|e| e.to_string())?.join(format!("{stem}.svg"));
        let n = flame::profile_pipeline(
            &src,
            &cli.input,
            Duration::from_secs_f64(cli.flame_secs),
            &svg,
        )?;
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
