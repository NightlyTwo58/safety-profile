/// Pipeline comparison harness: our pipeline vs Yosys.
///
/// Usage:
///   cargo run --bin compare --release                         # all of inputs/
///   cargo run --bin compare --release -- inputs/c17.v c432.v  # explicit files
///   cargo run --bin compare --release -- inputs -f c4 -i 50   # filter, 50 iters
///   cargo run --bin compare --release -- --flamegraph --csv
///
/// Ours is a direct library call (typed Durations, no subprocess).  Yosys is a
/// subprocess, so its wall time includes interpreter startup.  We measure that
/// startup once (`yosys-net` = yosys minus startup) and report both speedups.
///
/// Artifacts go to `<out-dir>` (default `output/`):
///   stats/compare.csv         one row per circuit (medians, sd, speedups)
///   stats/compare_stages.csv  one row per circuit x stage (with % of total)
///   flamegraphs/<circuit>.svg with --flamegraph
///   verilog/{ours,yosys}/     last emitted netlists, for diffing
use clap::Parser;
use safety_profiler::{
    flame,
    io::{collect_inputs, OutDir},
    run_pipeline,
    stats::Summary,
    STAGE_NAMES,
};
use std::{
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode, Stdio},
    time::{Duration, Instant},
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Verilog files and/or directories to benchmark.
    #[arg(value_name = "PATH", default_value = "inputs")]
    paths: Vec<PathBuf>,

    /// Only run circuits whose file stem contains this substring.
    #[arg(short, long)]
    filter: Option<String>,

    /// Root directory for generated artifacts.
    #[arg(short, long, default_value = "output")]
    out_dir: PathBuf,

    /// Timed iterations per circuit and tool.
    #[arg(short, long, default_value_t = 20)]
    iters: usize,

    /// Untimed warmup iterations per circuit and tool.
    #[arg(long, default_value_t = 3)]
    warmup: usize,

    /// Print CSV to stdout instead of the table (files are always written).
    #[arg(long)]
    csv: bool,

    /// Skip Yosys (ours only).
    #[arg(long)]
    no_yosys: bool,

    /// Write a per-circuit flamegraph (looped for --flame-secs).
    #[arg(long)]
    flamegraph: bool,

    /// Wall-clock budget per flamegraph, in seconds.
    #[arg(long, default_value_t = 2.0)]
    flame_secs: f64,
}

// Yosys

/// Run yosys once with `script`; returns wall time in microseconds.
fn yosys_once(script: &str) -> Result<u128, String> {
    let t = Instant::now();
    let status = Command::new("yosys")
        .args(["-q", "-p", script])
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;
    let us = t.elapsed().as_micros();
    if status.success() { Ok(us) } else { Err(format!("yosys exited with {status}")) }
}

fn yosys_script(input: &Path, output: &Path) -> String {
    format!(
        "read_verilog {}; opt_expr; opt_clean; write_verilog -noattr {}",
        input.display(),
        output.display()
    )
}

/// Median cost of launching yosys and running a no-op command.
fn yosys_startup(iters: usize) -> Result<f64, String> {
    let samples = (0..iters.max(3))
        .map(|_| yosys_once("design -reset"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples).median)
}

// Measurement

struct Row {
    name: String,
    stages: Vec<Summary>, // same order as STAGE_NAMES
    ours: Summary,        // per-run total
    yosys: Option<Summary>,
}

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den > 0.0).then(|| num / den)
}

fn fmt_ratio(r: Option<f64>) -> String {
    r.map_or("n/a".into(), |r| format!("{r:.2}x"))
}

impl Row {
    fn speedup_raw(&self) -> Option<f64> {
        ratio(self.yosys?.median, self.ours.median)
    }
    fn yosys_net(&self, startup: f64) -> Option<f64> {
        self.yosys.map(|y| (y.median - startup).max(0.0))
    }
    fn speedup_net(&self, startup: f64) -> Option<f64> {
        ratio(self.yosys_net(startup)?, self.ours.median)
    }
}

fn measure_ours(src: &str, path: &Path, cli: &Cli) -> Result<(Vec<Summary>, Summary, String), String> {
    let run = || run_pipeline(src, path).map_err(|e| format!("pipeline error: {e}"));
    for _ in 0..cli.warmup {
        run()?;
    }
    let iters = cli.iters.max(1);
    let mut per_stage = vec![Vec::with_capacity(iters); STAGE_NAMES.len()];
    let mut totals = Vec::with_capacity(iters);
    let mut verilog = String::new();
    for _ in 0..iters {
        let r = run()?;
        for (samples, name) in per_stage.iter_mut().zip(STAGE_NAMES) {
            samples.push(r.stage(name).micros());
        }
        totals.push(r.total_micros());
        verilog = r.verilog;
    }
    let stages = per_stage.iter().map(|s| Summary::from_micros(s)).collect();
    Ok((stages, Summary::from_micros(&totals), verilog))
}

fn measure_yosys(input: &Path, output: &Path, cli: &Cli) -> Result<Summary, String> {
    let script = yosys_script(input, output);
    for _ in 0..cli.warmup {
        yosys_once(&script)?;
    }
    let samples = (0..cli.iters.max(1))
        .map(|_| yosys_once(&script))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples))
}

// Output formatting

fn print_table_header() {
    println!("=== Pipeline Comparison (median µs over timed iterations) ===\n");
    print!("{:<18}", "circuit");
    for s in STAGE_NAMES {
        print!(" {s:>9}");
    }
    println!(" {:>10} {:>10} {:>10} {:>8} {:>8}", "ours", "yosys", "yosys-net", "x-raw", "x-net");
    println!("{}", "─".repeat(18 + 10 * STAGE_NAMES.len() + 10 * 3 + 8 * 2 + 5));
}

fn print_table_row(r: &Row, startup: f64) {
    print!("{:<18}", r.name);
    for s in &r.stages {
        print!(" {:>9.1}", s.median);
    }
    let y = |v: Option<f64>| v.map_or("n/a".into(), |v| format!("{v:.1}"));
    println!(
        " {:>10.1} {:>10} {:>10} {:>8} {:>8}",
        r.ours.median,
        y(r.yosys.map(|s| s.median)),
        y(r.yosys_net(startup)),
        fmt_ratio(r.speedup_raw()),
        fmt_ratio(r.speedup_net(startup)),
    );
}

fn summary_csv_header() -> String {
    let mut h = String::from("circuit");
    for s in STAGE_NAMES {
        let _ = write!(h, ",{s}_us");
    }
    h.push_str(",ours_median_us,ours_min_us,ours_sd_us,yosys_median_us,yosys_sd_us,yosys_startup_us,speedup_raw,speedup_net,iters\n");
    h
}

fn summary_csv_row(r: &Row, startup: f64) -> String {
    let mut line = r.name.clone();
    for s in &r.stages {
        let _ = write!(line, ",{:.1}", s.median);
    }
    let opt = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.1}"));
    let ratio_csv = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.3}"));
    let _ = writeln!(
        line,
        ",{:.1},{:.1},{:.1},{},{},{:.1},{},{},{}",
        r.ours.median,
        r.ours.min,
        r.ours.stddev,
        opt(r.yosys.map(|s| s.median)),
        opt(r.yosys.map(|s| s.stddev)),
        startup,
        ratio_csv(r.speedup_raw()),
        ratio_csv(r.speedup_net(startup)),
        r.ours.n,
    );
    line
}

const STAGES_CSV_HEADER: &str = "circuit,stage,median_us,min_us,sd_us,pct_of_total\n";

fn stages_csv_rows(r: &Row) -> String {
    let sum: f64 = r.stages.iter().map(|s| s.median).sum();
    let mut out = String::new();
    for (name, s) in STAGE_NAMES.iter().zip(&r.stages) {
        let pct = if sum > 0.0 { 100.0 * s.median / sum } else { 0.0 };
        let _ = writeln!(out, "{},{name},{:.1},{:.1},{:.1},{pct:.1}", r.name, s.median, s.min, s.stddev);
    }
    out
}

// Entry point

fn run(cli: Cli) -> Result<usize, String> {
    let inputs = collect_inputs(&cli.paths, cli.filter.as_deref())?;
    let out = OutDir::new(&cli.out_dir);
    let ours_dir = out.verilog("ours").map_err(|e| e.to_string())?;
    let yosys_dir = out.verilog("yosys").map_err(|e| e.to_string())?;
    let stats_dir = out.stats().map_err(|e| e.to_string())?;

    let startup = if cli.no_yosys { 0.0 } else { yosys_startup(cli.iters)? };

    if cli.csv {
        print!("{}", summary_csv_header());
    } else {
        print_table_header();
    }

    let mut summary_csv = summary_csv_header();
    let mut stages_csv = String::from(STAGES_CSV_HEADER);
    let mut failures = 0usize;

    for input in &inputs {
        let name = input.file_stem().unwrap_or_default().to_string_lossy().into_owned();
        let mut fail = |msg: String| {
            eprintln!("  {name}: {msg}");
            failures += 1;
        };

        let src = match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => { fail(format!("read error: {e}")); continue; }
        };

        let (stages, ours, verilog) = match measure_ours(&src, input, &cli) {
            Ok(v) => v,
            Err(e) => { fail(e); continue; }
        };
        if let Err(e) = fs::write(ours_dir.join(format!("{name}.v")), &verilog) {
            eprintln!("  {name}: could not save our output: {e}");
        }

        let yosys = if cli.no_yosys {
            None
        } else {
            match measure_yosys(input, &yosys_dir.join(format!("{name}.v")), &cli) {
                Ok(s) => Some(s),
                Err(e) => { fail(format!("yosys error: {e}")); continue; }
            }
        };

        let row = Row { name: name.clone(), stages, ours, yosys };
        if cli.csv {
            print!("{}", summary_csv_row(&row, startup));
        } else {
            print_table_row(&row, startup);
        }
        summary_csv.push_str(&summary_csv_row(&row, startup));
        stages_csv.push_str(&stages_csv_rows(&row));

        // Profile after timing so the sampler never perturbs measurements.
        if cli.flamegraph {
            let svg = out.flamegraphs().map_err(|e| e.to_string())?.join(format!("{name}.svg"));
            match flame::profile_pipeline(&src, input, Duration::from_secs_f64(cli.flame_secs), &svg) {
                Ok(n) => eprintln!("  {name}: flamegraph ({n} iterations) -> {}", svg.display()),
                Err(e) => eprintln!("  {name}: flamegraph failed: {e}"),
            }
        }
    }

    for (file, body) in [("compare.csv", &summary_csv), ("compare_stages.csv", &stages_csv)] {
        let p = stats_dir.join(file);
        fs::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))?;
    }
    eprintln!("\nStats written to {}", stats_dir.display());
    if !cli.no_yosys {
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
