/// ISCAS-85 Verilog preprocessor.
///
/// Usage:
///   cargo run --bin preprocess --release -- [RAW_DIR] [OUT_DIR] [--techmap FILE]
///   cargo run --bin preprocess --release      # defaults: inputs/raw -> inputs/
use clap::Parser;
use eqmap::analysis::LutAnalysis;
use eqmap::driver::{SynthReport, SynthRequest};
use eqmap::lut::LutLang;
use eqmap::rewrite::lutpacking_rules;
use eqmap::verilog::{sv_parse_wrapper, SVModule};
use safety_profiler::io::collect_inputs;

use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, ExitCode},
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Directory with unmodified ISCAS-85 .v files.
    #[arg(default_value = "inputs/raw")]
    raw_dir: PathBuf,

    /// Where normalized + synthesized files are written.
    #[arg(default_value = "inputs")]
    out_dir: PathBuf,

    /// Techmap file (default: <OUT_DIR>/techmap.v).
    #[arg(long)]
    techmap: Option<PathBuf>,

    /// LUT size for eqmap.
    #[arg(short, long, default_value_t = 6)]
    k: usize,
}

/// Run yosys to normalize ISCAS-85 primitives into named CellType instances.
fn run_yosys(input: &Path, output: &Path, techmap: &Path) -> Result<(), String> {
    let script = format!(
        "read_verilog {}; proc; techmap -map {}; clean -purge; write_verilog -noattr {}",
        input.display(),
        techmap.display(),
        output.display(),
    );

    let result = Command::new("yosys")
        .args(["-q", "-p", &script])
        .output()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("yosys exited with {}\n{stderr}", result.status));
    }
    Ok(())
}

/// Parse the normalized Verilog, run eqmap's synthesis pipeline, and write
/// the resulting Verilog back out.
fn run_eqmap(normalized: &Path, output: &Path, k: usize) -> Result<(), String> {
    let src = fs::read_to_string(normalized).map_err(|e| format!("failed to read file: {e}"))?;

    let ast = sv_parse_wrapper(&src, Some(normalized.to_path_buf()))
        .map_err(|e| format!("verilog parse error: {e}"))?;

    let module = SVModule::from_ast(&ast)?;
    let mod_name = module.get_name().to_string();

    // Handles both single- and multi-output modules (e.g. c17.v), unlike
    // to_expr::<LutLang>() which requires exactly one output.
    let expr = module.to_single_lut_expr()?;

    let mut req: SynthRequest<LutLang, LutAnalysis> = SynthRequest::default()
        .with_expr(expr)
        .with_k(k)
        .with_rules(lutpacking_rules())
        .without_progress_bar();

    let result = req
        .synth::<SynthReport>()
        .map_err(|e| format!("eqmap synthesis failed: {e}"))?;

    let out_module = SVModule::from_luts(result.get_expr().clone(), mod_name, vec![])?;

    fs::write(output, out_module.to_string()).map_err(|e| format!("failed to write output: {e}"))
}

fn process_one(input: &Path, output: &Path, techmap: &Path, k: usize) -> Result<(), String> {
    // Normalize into a temp file next to the final output, then run eqmap
    // over it and overwrite output with eqmap's result.  The temp file is
    // removed on both success and failure.
    let normalized = output.with_extension("normalized.v");
    let result = run_yosys(input, &normalized, techmap).and_then(|()| run_eqmap(&normalized, output, k));
    let _ = fs::remove_file(&normalized);
    result
}

fn run(cli: Cli) -> Result<usize, String> {
    let techmap = cli.techmap.clone().unwrap_or_else(|| cli.out_dir.join("techmap.v"));

    if !cli.raw_dir.exists() {
        return Err(format!(
            "'{}' not found.\nPlace unmodified ISCAS-85 .v files there and re-run.",
            cli.raw_dir.display()
        ));
    }
    if !techmap.exists() {
        return Err(format!(
            "'{}' not found.\nCopy inputs/techmap.v there or pass --techmap.",
            techmap.display()
        ));
    }
    fs::create_dir_all(&cli.out_dir).map_err(|e| format!("cannot create output directory: {e}"))?;

    println!("=== Preprocessing ISCAS-85 inputs (yosys/techmap.v + eqmap synth) ===");
    println!("  raw:     {}", cli.raw_dir.display());
    println!("  output:  {}", cli.out_dir.display());
    println!("  techmap: {}\n", techmap.display());

    let inputs = collect_inputs(std::slice::from_ref(&cli.raw_dir), None)?;
    let mut failures = 0usize;

    for input in &inputs {
        let file = input.file_name().unwrap();
        let name = file.to_string_lossy();
        let output = cli.out_dir.join(file);

        match process_one(input, &output, &techmap, cli.k) {
            Ok(()) => println!("  {name}: OK -> {}", output.display()),
            Err(e) => {
                eprintln!("  {name}: FAILED\n    {e}");
                failures += 1;
            }
        }
    }
    Ok(failures)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(0) => {
            println!("\nDone. Run:  cargo run --bin compare --release");
            ExitCode::SUCCESS
        }
        Ok(n) => {
            eprintln!("\n{n} file(s) failed.");
            ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}
