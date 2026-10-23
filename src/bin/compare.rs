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
///   stats/compare_rename.csv  isolated sequential-renaming comparison
///   flamegraphs/<circuit>.pipeline.svg with --flamegraph
///   verilog/{ours,yosys}/     last emitted netlists, for diffing
use clap::Parser;
use safety_profiler::{
    STAGE_NAMES, flame,
    io::{OutDir, collect_inputs, exclude_inputs},
    run_pipeline, run_rename_nets,
    stats::Summary,
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

    /// Skip inputs whose file stem contains this substring (repeatable).
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

    /// Print the pipeline CSV to stdout instead of its table; stats files are also written.
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
    if status.success() {
        Ok(us)
    } else {
        Err(format!("yosys exited with {status}"))
    }
}

fn yosys_script(input: &Path, output: &Path) -> String {
    format!(
        "read_verilog {}; opt_expr; opt_clean; write_verilog -noattr {}",
        yosys_quote(input),
        yosys_quote(output)
    )
}

fn yosys_quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

/// Estimate how many rename pairs to run to get useful Yosys timing resolution.
/// Large designs need fewer repetitions because each pass already takes longer.
fn rename_rounds(input: &Path) -> Result<usize, String> {
    let bytes = fs::metadata(input)
        .map_err(|e| format!("{}: {e}", input.display()))?
        .len()
        .max(1);
    Ok(((16 * 1024 * 1024) / bytes).clamp(1, 1_000) as usize)
}

fn yosys_rename_script(input: &Path, rounds: usize) -> String {
    let mut script = format!(
        "read_verilog {}; design -save profiler_rename_base; ",
        yosys_quote(input)
    );
    for _ in 0..rounds {
        script.push_str(
            "design -load profiler_rename_base; rename -hide *; rename -enumerate -pattern __%__ *; ",
        );
    }
    script
}

/// Measure Yosys's rename passes internally, excluding parse and process startup.
fn yosys_rename_once(input: &Path, rounds: usize) -> Result<u128, String> {
    let script = yosys_rename_script(input, rounds);
    let result = Command::new("yosys")
        .args(["-Q", "-d", "-p", &script])
        .output()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;
    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("yosys exited with {}\n{stderr}", result.status));
    }

    let stdout = String::from_utf8_lossy(&result.stdout);
    let stderr = String::from_utf8_lossy(&result.stderr);
    let report = format!("{stdout}\n{stderr}");
    let seconds = report
        .split("Time spent:")
        .last()
        .and_then(|timings| {
            timings.lines().find_map(|line| {
                let fields: Vec<_> = line.split_whitespace().collect();
                if fields.last() != Some(&"rename") {
                    return None;
                }
                let sec = fields.iter().position(|field| *field == "sec")?;
                fields.get(sec.checked_sub(1)?)?.parse::<f64>().ok()
            })
        })
        .ok_or_else(|| "could not read rename timing from Yosys -d output".to_string())?;
    Ok((seconds * 1_000_000.0 / rounds as f64).round() as u128)
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
    rename_ours: Summary,
    rename_yosys: Option<Summary>,
    rename_rounds: usize,
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

    fn rename_speedup(&self) -> Option<f64> {
        ratio(self.rename_yosys?.median, self.rename_ours.median)
    }
}

fn measure_ours(
    src: &str,
    path: &Path,
    cli: &Cli,
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

fn measure_ours_rename(src: &str, path: &Path, cli: &Cli) -> Result<Summary, String> {
    let run = || run_rename_nets(src, path).map_err(|e| format!("rename nets error: {e}"));
    for _ in 0..cli.warmup {
        run()?;
    }
    let samples = (0..cli.iters.max(1))
        .map(|_| run().map(|stage| stage.micros()))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples))
}

fn measure_yosys_rename(input: &Path, rounds: usize, cli: &Cli) -> Result<Summary, String> {
    for _ in 0..cli.warmup {
        yosys_rename_once(input, rounds)?;
    }
    let samples = (0..cli.iters.max(1))
        .map(|_| yosys_rename_once(input, rounds))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples))
}

// Output formatting

fn print_table_header(include_yosys: bool) {
    println!("=== Pipeline Comparison (median µs over timed iterations) ===\n");
    print!("{:<18}", "circuit");
    for s in STAGE_NAMES {
        print!(" {s:>9}");
    }
    if include_yosys {
        println!(
            " {:>10} {:>10} {:>10} {:>8} {:>8}",
            "ours", "yosys", "yosys-net", "x-raw", "x-net"
        );
    } else {
        println!(" {:>10}", "ours");
    }
    let columns = if include_yosys { 10 * 3 + 8 * 2 } else { 10 };
    println!("{}", "─".repeat(18 + 10 * STAGE_NAMES.len() + columns + 5));
}

fn print_table_row(r: &Row, startup: f64, include_yosys: bool) {
    print!("{:<18}", r.name);
    for s in &r.stages {
        print!(" {:>9.1}", s.median);
    }
    let y = |v: Option<f64>| v.map_or("n/a".into(), |v| format!("{v:.1}"));
    if include_yosys {
        println!(
            " {:>10.1} {:>10} {:>10} {:>8} {:>8}",
            r.ours.median,
            y(r.yosys.map(|s| s.median)),
            y(r.yosys_net(startup)),
            fmt_ratio(r.speedup_raw()),
            fmt_ratio(r.speedup_net(startup)),
        );
    } else {
        println!(" {:>10.1}", r.ours.median);
    }
}

fn summary_csv_header(include_yosys: bool) -> String {
    let mut h = String::from("circuit");
    for s in STAGE_NAMES {
        let _ = write!(h, ",{s}_us");
    }
    if include_yosys {
        h.push_str(",ours_median_us,ours_min_us,ours_sd_us,yosys_median_us,yosys_sd_us,yosys_startup_us,speedup_raw,speedup_net,iters\n");
    } else {
        h.push_str(",ours_median_us,ours_min_us,ours_sd_us,iters\n");
    }
    h
}

fn summary_csv_row(r: &Row, startup: f64, include_yosys: bool) -> String {
    let mut line = r.name.clone();
    for s in &r.stages {
        let _ = write!(line, ",{:.1}", s.median);
    }
    let opt = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.1}"));
    let ratio_csv = |v: Option<f64>| v.map_or(String::new(), |v| format!("{v:.3}"));
    if include_yosys {
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
    } else {
        let _ = writeln!(
            line,
            ",{:.1},{:.1},{:.1},{}",
            r.ours.median, r.ours.min, r.ours.stddev, r.ours.n
        );
    }
    line
}

const STAGES_CSV_HEADER: &str = "circuit,stage,median_us,min_us,sd_us,pct_of_total\n";

fn stages_csv_rows(r: &Row) -> String {
    let sum: f64 = r.stages.iter().map(|s| s.median).sum();
    let mut out = String::new();
    for (name, s) in STAGE_NAMES.iter().zip(&r.stages) {
        let pct = if sum > 0.0 {
            100.0 * s.median / sum
        } else {
            0.0
        };
        let _ = writeln!(
            out,
            "{},{name},{:.1},{:.1},{:.1},{pct:.1}",
            r.name, s.median, s.min, s.stddev
        );
    }
    out
}

fn print_rename_table(rows: &[Row], include_yosys: bool) {
    println!("\n=== Sequential RenameNets comparison (µs per operation) ===");
    print!(
        "{:<18} {:>12} {:>10} {:>10}",
        "circuit", "ours median", "ours min", "ours sd"
    );
    if include_yosys {
        print!(
            " {:>16} {:>10} {:>10} {:>11}",
            "Yosys median", "Yosys min", "Yosys sd", "Yosys/ours"
        );
    }
    println!();
    for row in rows {
        print!(
            "{:<18} {:>12.1} {:>10.1} {:>10.1}",
            row.name, row.rename_ours.median, row.rename_ours.min, row.rename_ours.stddev
        );
        if include_yosys {
            let yosys = row.rename_yosys.expect("measured when Yosys is enabled");
            println!(
                " {:>16.1} {:>10.1} {:>10.1} {:>8}",
                yosys.median,
                yosys.min,
                yosys.stddev,
                fmt_ratio(row.rename_speedup()),
            );
        } else {
            println!();
        }
    }
}

fn rename_csv_header(include_yosys: bool) -> String {
    if include_yosys {
        String::from(
            "circuit,ours_median_us,ours_min_us,ours_sd_us,yosys_hide_enumerate_median_us,yosys_min_us,yosys_sd_us,yosys_over_ours,iters,yosys_pairs_per_sample\n",
        )
    } else {
        String::from("circuit,ours_median_us,ours_min_us,ours_sd_us,iters\n")
    }
}

fn rename_csv_row(row: &Row, include_yosys: bool) -> String {
    if let Some(yosys) = row.rename_yosys.filter(|_| include_yosys) {
        format!(
            "{},{:.1},{:.1},{:.1},{:.1},{:.1},{:.1},{:.3},{},{}\n",
            row.name,
            row.rename_ours.median,
            row.rename_ours.min,
            row.rename_ours.stddev,
            yosys.median,
            yosys.min,
            yosys.stddev,
            row.rename_speedup().unwrap_or_default(),
            row.rename_ours.n,
            row.rename_rounds,
        )
    } else {
        format!(
            "{},{:.1},{:.1},{:.1},{}\n",
            row.name,
            row.rename_ours.median,
            row.rename_ours.min,
            row.rename_ours.stddev,
            row.rename_ours.n,
        )
    }
}

// Entry point

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
    let mut ours_dir: Option<PathBuf> = None;
    let mut yosys_dir: Option<PathBuf> = None;

    let startup = if cli.no_yosys {
        0.0
    } else {
        yosys_startup(cli.iters)?
    };

    let include_yosys = !cli.no_yosys;
    if cli.csv {
        print!("{}", summary_csv_header(include_yosys));
    } else {
        print_table_header(include_yosys);
    }

    let mut summary_csv = summary_csv_header(include_yosys);
    let mut stages_csv = String::from(STAGES_CSV_HEADER);
    let mut rename_csv = rename_csv_header(include_yosys);
    let mut failures = 0usize;
    let mut completed = 0usize;
    let mut rows = Vec::new();

    for input in &inputs {
        let name = input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned();
        let mut fail = |msg: String| {
            eprintln!("  {name}: {msg}");
            failures += 1;
        };

        let src = match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => {
                fail(format!("read error: {e}"));
                continue;
            }
        };

        let (stages, ours, verilog) = match measure_ours(&src, input, &cli) {
            Ok(v) => v,
            Err(e) => {
                fail(e);
                continue;
            }
        };
        let ours_dir = match &ours_dir {
            Some(dir) => dir.clone(),
            None => {
                let dir = out.verilog("ours").map_err(|e| e.to_string())?;
                ours_dir = Some(dir.clone());
                dir
            }
        };
        if let Err(e) = fs::write(ours_dir.join(format!("{name}.v")), &verilog) {
            eprintln!("  {name}: could not save our output: {e}");
        }

        let rename_ours = match measure_ours_rename(&src, input, &cli) {
            Ok(summary) => summary,
            Err(e) => {
                fail(e);
                continue;
            }
        };
        let rename_rounds = if cli.no_yosys {
            0
        } else {
            rename_rounds(input)?
        };

        let (yosys, rename_yosys) = if cli.no_yosys {
            (None, None)
        } else {
            let dir = match &yosys_dir {
                Some(dir) => dir.clone(),
                None => {
                    let dir = out.verilog("yosys").map_err(|e| e.to_string())?;
                    yosys_dir = Some(dir.clone());
                    dir
                }
            };
            let pipeline = match measure_yosys(input, &dir.join(format!("{name}.v")), &cli) {
                Ok(s) => s,
                Err(e) => {
                    fail(format!("yosys error: {e}"));
                    continue;
                }
            };
            let rename = match measure_yosys_rename(input, rename_rounds, &cli) {
                Ok(s) => s,
                Err(e) => {
                    fail(format!("yosys rename error: {e}"));
                    continue;
                }
            };
            (Some(pipeline), Some(rename))
        };

        let row = Row {
            name: name.clone(),
            stages,
            ours,
            yosys,
            rename_ours,
            rename_yosys,
            rename_rounds,
        };
        if cli.csv {
            print!("{}", summary_csv_row(&row, startup, include_yosys));
        } else {
            print_table_row(&row, startup, include_yosys);
        }
        summary_csv.push_str(&summary_csv_row(&row, startup, include_yosys));
        stages_csv.push_str(&stages_csv_rows(&row));
        rename_csv.push_str(&rename_csv_row(&row, include_yosys));
        completed += 1;

        // Profile after timing so the sampler never perturbs measurements.
        if cli.flamegraph {
            let svg = out.pipeline_flamegraph(&name).map_err(|e| e.to_string())?;
            match flame::profile_pipeline(
                &src,
                input,
                Duration::from_secs_f64(cli.flame_secs),
                &svg,
            ) {
                Ok(n) => eprintln!("  {name}: flamegraph ({n} iterations) -> {}", svg.display()),
                Err(e) => eprintln!("  {name}: flamegraph failed: {e}"),
            }
        }
        rows.push(row);
    }

    if completed > 0 {
        if !cli.csv {
            print_rename_table(&rows, include_yosys);
        }
        let stats_dir = out.stats().map_err(|e| e.to_string())?;
        for (file, body) in [
            ("compare.csv", &summary_csv),
            ("compare_stages.csv", &stages_csv),
            ("compare_rename.csv", &rename_csv),
        ] {
            let p = stats_dir.join(file);
            fs::write(&p, body).map_err(|e| format!("{}: {e}", p.display()))?;
        }
        eprintln!("\nStats written to {}", stats_dir.display());
    } else {
        eprintln!("\nNo successful comparisons; no stats files written.");
    }
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
