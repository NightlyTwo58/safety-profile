/// ISCAS-85 Verilog preprocessor.
///
/// Usage:
///   cargo run --bin preprocess --release -- [raw_dir] [out_dir]
///   cargo run --bin preprocess --release           # defaults: inputs/raw → inputs/

use eqmap::analysis::LutAnalysis;
use eqmap::driver::{SynthRequest, SynthReport};
use eqmap::lut::LutLang;
use eqmap::rewrite::lutpacking_rules;
use eqmap::verilog::{SVModule, sv_parse_wrapper};

use std::{fs, path::PathBuf, process::Command};

/// Run yosys to normalize ISCAS-85 primitives into named CellType instances.
fn run_yosys(
    input: &std::path::Path,
    output: &std::path::Path,
    techmap: &std::path::Path,
) -> Result<(), String> {
    let script = format!(
        "read_verilog {input}; proc; techmap -map {techmap}; clean -purge; write_verilog -noattr {output}",
        input = input.display(),
        output = output.display(),
        techmap = techmap.display(),
    );

    let result = Command::new("yosys")
        .args(["-q", "-p", &script])
        .output()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;

    if !result.status.success() {
        let stderr = String::from_utf8_lossy(&result.stderr);
        return Err(format!("yosys exited with {}\n{}", result.status, stderr));
    }

    Ok(())
}

/// Parse the normalized Verilog, run eqmap's synthesis pipeline, and write
/// the resulting Verilog back out.
fn run_eqmap(normalized: &std::path::Path, output: &std::path::Path) -> Result<(), String> {
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
        .with_k(6) // matches eqmap_fpga's `-k` default
        .with_rules(lutpacking_rules())
        .without_progress_bar();

    let result = req
        .synth::<SynthReport>()
        .map_err(|e| format!("eqmap synthesis failed: {e}"))?;

    let out_module = SVModule::from_luts(result.get_expr().clone(), mod_name, vec![])?;

    fs::write(output, out_module.to_string())
        .map_err(|e| format!("failed to write output: {e}"))?;

    Ok(())
}

fn process_one(
    input: &std::path::Path,
    output: &std::path::Path,
    techmap: &std::path::Path,
) -> Result<(), String> {
    // Normalize into a temp file next to the final output, then run eqmap
    // over the normalized version and overwrite output with eqmap's result.
    let normalized = output.with_extension("normalized.v");
    run_yosys(input, &normalized, techmap)?;
    let result = run_eqmap(&normalized, output);
    let _ = fs::remove_file(&normalized);
    result
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw_dir = PathBuf::from(args.get(1).map(String::as_str).unwrap_or("inputs/raw"));
    let out_dir = PathBuf::from(args.get(2).map(String::as_str).unwrap_or("inputs"));
    // techmap.v lives next to the output files so it's easy to find and edit.
    let techmap = out_dir.join("techmap.v");

    if !raw_dir.exists() {
        eprintln!(
            "ERROR: '{}' not found.\nPlace unmodified ISCAS-85 .v files there and re-run.",
            raw_dir.display()
        );
        std::process::exit(1);
    }

    if !techmap.exists() {
        eprintln!(
            "ERROR: '{}' not found.\nCopy inputs/techmap.v into the output directory.",
            techmap.display()
        );
        std::process::exit(1);
    }

    fs::create_dir_all(&out_dir).expect("Failed to create output directory");

    println!("=== Preprocessing ISCAS-85 inputs (yosys/techmap.v + eqmap synth) ===");
    println!("  raw:     {}", raw_dir.display());
    println!("  output:  {}", out_dir.display());
    println!("  techmap: {}\n", techmap.display());

    let mut inputs: Vec<PathBuf> = fs::read_dir(&raw_dir)
        .expect("Failed to read raw dir")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().map_or(false, |e| e == "v"))
        .collect();
    inputs.sort();

    if inputs.is_empty() {
        eprintln!("No .v files found in {}/", raw_dir.display());
        std::process::exit(1);
    }

    let mut failures = 0usize;

    for input in &inputs {
        let name = input.file_name().unwrap().to_string_lossy();
        let output = out_dir.join(input.file_name().unwrap());

        match process_one(input, &output, &techmap) {
            Ok(()) => println!("  {name}: OK → {}", output.display()),
            Err(e) => {
                eprintln!("  {name}: FAILED\n    {e}");
                failures += 1;
            }
        }
    }

    println!();
    if failures == 0 {
        println!("Done. Run:  cargo run --bin compare --release");
    } else {
        eprintln!("{failures} file(s) failed.");
        std::process::exit(1);
    }
}