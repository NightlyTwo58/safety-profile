//! Rename nets and instances sequentially and write the renamed netlists.
//!
//! Output goes to `<out-dir>/verilog/renamed/<circuit>.v`.

use clap::Parser;
use safety_profiler::{
    io::{collect_inputs, exclude_inputs, stem, OutDir},
    rename_netlist,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

/// Rename nets and instances in Verilog netlists.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Verilog files and/or directories to rename.
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

fn rename_one(input: &Path, dir: &Path) -> Result<PathBuf, String> {
    let src = safety_profiler::io::read_source(input).map_err(|e| format!("read error: {e}"))?;
    let verilog = rename_netlist(&src, input).map_err(|e| format!("rename failed: {e}"))?;
    let path = dir.join(format!("{}.v", stem(input)));
    fs::write(&path, verilog).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

fn run(cli: Cli) -> Result<usize, String> {
    let mut inputs = collect_inputs(&cli.paths, cli.filter.as_deref())?;
    exclude_inputs(&mut inputs, &cli.excludes);
    let dir = OutDir::new(&cli.out_dir)
        .verilog("renamed")
        .map_err(|e| e.to_string())?;

    let mut failures = 0;
    for input in &inputs {
        match rename_one(input, &dir) {
            Ok(path) => println!("  {}: OK -> {}", stem(input), path.display()),
            Err(e) => {
                eprintln!("  {}: {e}", stem(input));
                failures += 1;
            }
        }
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
