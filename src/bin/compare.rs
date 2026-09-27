/// Pipeline comparison harness.
///
/// Usage:
///   cargo run --bin compare --release
///   cargo run --bin compare --release -- --csv
///
/// For each .v file in inputs/, runs our pipeline via `run_pipeline()` (no
/// subprocess — direct library call) and Yosys via `std::process::Command`.
/// Prints a side-by-side timing table or CSV.
///
/// Why no subprocess for ours:
///   Spawning `safety-profiler` as a child process and scraping its stdout
///   adds ~1-5ms of OS overhead and requires parsing text we generated
///   ourselves.  Calling `run_pipeline()` gives us typed `Duration` values
///   with zero overhead and no parsing.
use safety_profiler::{run_pipeline, PipelineResult};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Instant,
};

//  Yosys runner 

struct YosysResult {
    elapsed_us: u128,
}

/// Spawn yosys with the equivalent optimization flow and measure wall time.
///
/// Note: this includes yosys interpreter startup (~100–300 µs on fast machines,
/// up to several ms on slow ones).  We document this rather than try to subtract
/// it, because it reflects real CLI usage and the effect shrinks on large circuits
/// where pass time dominates.
fn run_yosys(input: &Path, output: &Path) -> Result<YosysResult, String> {
    fs::create_dir_all(output.parent().unwrap()).map_err(|e| e.to_string())?;

    let script = format!(
        "read_verilog {input}; opt_expr; opt_clean; write_verilog -noattr {output}",
        input = input.display(),
        output = output.display(),
    );

    let t = Instant::now();
    let status = Command::new("yosys")
        .args(["-q", "-p", &script])
        .status()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;
    let elapsed_us = t.elapsed().as_micros();

    if !status.success() {
        return Err(format!("yosys exited with {status}"));
    }

    Ok(YosysResult { elapsed_us })
}

//  Table / CSV output 

const STAGE_NAMES: &[&str] = &["parse", "compile", "clean1", "fold", "clean2", "emit"];

fn print_header_table() {
    println!("=== Pipeline Comparison (all times in µs) ===\n");
    println!(
        "{:<20} {:>10} {:>10} {:>10} {:>10} {:>10} {:>10} {:>13} {:>13} {:>8}",
        "circuit", "parse", "compile", "clean1", "fold", "clean2", "emit",
        "ours-total", "yosys-total", "speedup"
    );
    println!("{}", "".repeat(116));
}

fn print_row_table(name: &str, ours: &PipelineResult, yosys_us: u128) {
    let ours_total = ours.total_micros();
    let speedup = if ours_total > 0 {
        format!("{:.2}x", yosys_us as f64 / ours_total as f64)
    } else {
        "n/a".into()
    };

    print!("{:<20}", name);
    for stage in STAGE_NAMES {
        print!(" {:>9}µs", ours.stage(stage).micros());
    }
    println!(" {:>12}µs {:>12}µs {:>8}", ours_total, yosys_us, speedup);
}

fn print_header_csv() {
    println!("circuit,parse_us,compile_us,clean1_us,fold_us,clean2_us,emit_us,ours_total_us,yosys_total_us,speedup");
}

fn print_row_csv(name: &str, ours: &PipelineResult, yosys_us: u128) {
    let ours_total = ours.total_micros();
    let speedup = if ours_total > 0 {
        format!("{:.2}x", yosys_us as f64 / ours_total as f64)
    } else {
        "n/a".into()
    };

    let stage_vals: Vec<String> = STAGE_NAMES
        .iter()
        .map(|n| ours.stage(n).micros().to_string())
        .collect();

    println!("{},{},{},{},{}", name, stage_vals.join(","), ours_total, yosys_us, speedup);
}

//  Entry point 

fn main() {
    let csv_mode = std::env::args().any(|a| a == "--csv");

    let inputs_dir = PathBuf::from("inputs");
    let yosys_out_dir = PathBuf::from("outputs/yosys");
    let ours_out_dir = PathBuf::from("outputs/ours");

    fs::create_dir_all(&yosys_out_dir).unwrap();
    fs::create_dir_all(&ours_out_dir).unwrap();

    // Collect and sort inputs so the table is deterministic.
    let mut inputs: Vec<PathBuf> = fs::read_dir(&inputs_dir)
        .unwrap_or_else(|_| panic!("inputs/ directory not found"))
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            p.extension().map_or(false, |e| e == "v")
                && !p.to_string_lossy().ends_with(".out.v")
        })
        .collect();
    inputs.sort();

    if inputs.is_empty() {
        eprintln!("No .v files found in {}/", inputs_dir.display());
        std::process::exit(1);
    }

    if csv_mode {
        print_header_csv();
    } else {
        print_header_table();
    }

    let mut failures = 0usize;

    for input in &inputs {
        let name = input.file_stem().unwrap().to_string_lossy();
        let src = match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => { eprintln!("  {name}: read error: {e}"); failures += 1; continue; }
        };

        //  Our pipeline (direct library call, no subprocess) 
        let ours = match run_pipeline(&src, input) {
            Ok(r) => r,
            Err(e) => { eprintln!("  {name}: pipeline error: {e}"); failures += 1; continue; }
        };

        // Save our output for later diffing.
        let ours_out = ours_out_dir.join(format!("{name}.v"));
        let _ = fs::write(&ours_out, &ours.verilog);

        //  Yosys (subprocess, wall-time measured with Instant) 
        let yosys_out = yosys_out_dir.join(format!("{name}.v"));
        let yosys_us = match run_yosys(input, &yosys_out) {
            Ok(r) => r.elapsed_us,
            Err(e) => { eprintln!("  {name}: yosys error: {e}"); failures += 1; continue; }
        };

        //  Emit row 
        if csv_mode {
            print_row_csv(&name, &ours, yosys_us);
        } else {
            print_row_table(&name, &ours, yosys_us);
        }
    }

    if failures > 0 {
        eprintln!("\n{failures} circuit(s) failed.");
        std::process::exit(1);
    }
}
