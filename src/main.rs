use safety_profiler::run_pipeline;
use std::{env, fs, path::PathBuf};

/// Print a timing line to stdout in a format compare can parse directly
/// (it never needs to — compare calls the lib — but this keeps the binary
/// usable standalone for quick eyeball checks).
///
/// Format: `[label]  <µs>µs  <optional note>`
fn print_stage(stage: &safety_profiler::StageTiming) {
    match &stage.note {
        Some(note) => println!("[{}]  {}µs  {}", stage.name, stage.micros(), note),
        None       => println!("[{}]  {}µs",     stage.name, stage.micros()),
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: safety-profiler <input.v>");
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    // File I/O is outside all timers — we measure pass performance, not disk.
    let src = fs::read_to_string(&path).expect("Failed to read input file");

    let result = run_pipeline(&src, &path).expect("Pipeline failed");

    for stage in &result.stages {
        print_stage(stage);
    }

    let out_path = path.with_extension("out.v");
    fs::write(&out_path, &result.verilog).expect("Failed to write output");
    eprintln!("Written to {}", out_path.display());
}
