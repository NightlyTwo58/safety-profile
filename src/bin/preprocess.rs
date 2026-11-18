//! Normalize raw Verilog into inputs valid for both tools.
//!
//! Yosys maps primitives to named cell instances via a techmap, then EqMap
//! packs the logic into LUTs.

use clap::Parser;
use eqmap::analysis::LutAnalysis;
use eqmap::driver::{SynthReport, SynthRequest};
use eqmap::lut::LutLang;
use eqmap::rewrite::lutpacking_rules;
use eqmap::verilog::{sv_parse_wrapper, SVModule};
use safety_profiler::{io::collect_inputs, yosys};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Normalize and LUT-pack raw ISCAS-85 Verilog files.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Directory with unmodified ISCAS-85 .v files.
    #[arg(default_value = "inputs/raw")]
    raw_dir: PathBuf,

    /// Directory for normalized and synthesized files.
    #[arg(default_value = "inputs")]
    out_dir: PathBuf,

    /// Techmap file [default: <OUT_DIR>/techmap.v].
    #[arg(long)]
    techmap: Option<PathBuf>,

    /// LUT size for EqMap.
    #[arg(short, long, default_value_t = 6)]
    k: usize,
}

/// Parse normalized Verilog, run EqMap LUT packing, and write the result.
///
/// Uses `to_single_lut_expr` so multi-output modules (e.g. c17) work.
fn pack_luts(normalized: &Path, output: &Path, k: usize) -> Result<(), String> {
    let src = fs::read_to_string(normalized).map_err(|e| format!("failed to read file: {e}"))?;
    let ast = sv_parse_wrapper(&src, Some(normalized.to_path_buf()))
        .map_err(|e| format!("verilog parse error: {e}"))?;

    let module = SVModule::from_ast(&ast)?;
    let name = module.get_name().to_string();
    let expr = module.to_single_lut_expr()?;

    let mut request: SynthRequest<LutLang, LutAnalysis> = SynthRequest::default()
        .with_expr(expr)
        .with_k(k)
        .with_rules(lutpacking_rules())
        .without_progress_bar();
    let result = request
        .synth::<SynthReport>()
        .map_err(|e| format!("eqmap synthesis failed: {e}"))?;

    let packed = SVModule::from_luts(result.get_expr().clone(), name, vec![])?;
    fs::write(output, packed.to_string()).map_err(|e| format!("failed to write output: {e}"))
}

/// Normalize `input` through a temporary file, then pack it into `output`.
///
/// The temporary file is removed on success and failure.
fn process_one(input: &Path, output: &Path, techmap: &Path, k: usize) -> Result<(), String> {
    let normalized = output.with_extension("normalized.v");
    let result = yosys::normalize(input, &normalized, techmap)
        .and_then(|()| pack_luts(&normalized, output, k));
    let _ = fs::remove_file(&normalized);
    result
}

/// Process every raw file; returns the number that failed.
fn run(cli: Cli) -> Result<usize, String> {
    let techmap = cli
        .techmap
        .clone()
        .unwrap_or_else(|| cli.out_dir.join("techmap.v"));

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
    let mut failures = 0;
    for input in &inputs {
        let file = input.file_name().expect("collected inputs are files");
        let output = cli.out_dir.join(file);
        match process_one(input, &output, &techmap, cli.k) {
            Ok(()) => println!("  {}: OK -> {}", file.to_string_lossy(), output.display()),
            Err(e) => {
                eprintln!("  {}: FAILED\n    {e}", file.to_string_lossy());
                failures += 1;
            }
        }
    }
    Ok(failures)
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(0) => {
            println!("\nDone. Run:  cargo run --release --bin compare");
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
