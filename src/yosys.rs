//! Yosys subprocess helpers: baseline timing, rename timing, normalization.

use crate::stats::Summary;
use std::{
    fs,
    path::Path,
    process::{Command, Output, Stdio},
    time::Instant,
};

/// Target bytes of design processed per rename sample; sets the repeat count.
const RENAME_BUDGET_BYTES: u64 = 16 * 1024 * 1024;

/// Quote a path for use inside a Yosys script.
pub fn quote(path: &Path) -> String {
    format!(
        "\"{}\"",
        path.to_string_lossy()
            .replace('\\', "\\\\")
            .replace('"', "\\\"")
    )
}

/// Run `script` once and return its wall time in microseconds.
///
/// Includes process startup. Stdout is discarded.
pub fn run(script: &str) -> Result<u128, String> {
    let start = Instant::now();
    let status = Command::new("yosys")
        .args(["-q", "-p", script])
        .stdout(Stdio::null())
        .status()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;
    let micros = start.elapsed().as_micros();
    if status.success() {
        Ok(micros)
    } else {
        Err(format!("yosys exited with {status}"))
    }
}

/// Run Yosys with `flags` and `script`, capturing its output.
fn capture(flags: &[&str], script: &str) -> Result<Output, String> {
    let output = Command::new("yosys")
        .args(flags)
        .args(["-p", script])
        .output()
        .map_err(|e| format!("failed to spawn yosys: {e}"))?;
    if output.status.success() {
        Ok(output)
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("yosys exited with {}\n{stderr}", output.status))
    }
}

/// Script for the baseline flow equivalent to this crate's pipeline.
pub fn pipeline_script(input: &Path, output: &Path) -> String {
    format!(
        "read_verilog {}; opt_expr; opt_clean; write_verilog -noattr {}",
        quote(input),
        quote(output)
    )
}

/// Median wall time of launching Yosys and running a no-op command.
pub fn startup(iters: usize) -> Result<f64, String> {
    let samples = (0..iters.max(3))
        .map(|_| run("design -reset"))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Summary::from_micros(&samples).median)
}

/// Number of rename pairs per sample, scaled inversely with design size.
pub fn rename_rounds(input: &Path) -> Result<usize, String> {
    let bytes = fs::metadata(input)
        .map_err(|e| format!("{}: {e}", input.display()))?
        .len()
        .max(1);
    Ok((RENAME_BUDGET_BYTES / bytes).clamp(1, 1_000) as usize)
}

/// Script that reloads the design and runs the rename pair `rounds` times.
fn rename_script(input: &Path, rounds: usize) -> String {
    let mut script = format!(
        "read_verilog {}; design -save profiler_rename_base; ",
        quote(input)
    );
    for _ in 0..rounds {
        script.push_str(
            "design -load profiler_rename_base; rename -hide *; rename -enumerate -pattern __%__ *; ",
        );
    }
    script
}

/// Seconds attributed to `rename` in a Yosys `-d` timing report.
fn rename_seconds(report: &str) -> Option<f64> {
    let timings = report.split("Time spent:").last()?;
    timings.lines().find_map(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        if fields.last() != Some(&"rename") {
            return None;
        }
        let sec = fields.iter().position(|f| *f == "sec")?;
        fields.get(sec.checked_sub(1)?)?.parse().ok()
    })
}

/// Microseconds per rename pair, read from Yosys's own timing report.
///
/// Excludes parsing and process startup.
pub fn rename_micros(input: &Path, rounds: usize) -> Result<u128, String> {
    let output = capture(&["-Q", "-d"], &rename_script(input, rounds))?;
    let report = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let seconds =
        rename_seconds(&report).ok_or("could not read rename timing from Yosys -d output")?;
    Ok((seconds * 1_000_000.0 / rounds as f64).round() as u128)
}

/// Normalize raw primitives into named cell instances via `techmap`.
pub fn normalize(input: &Path, output: &Path, techmap: &Path) -> Result<(), String> {
    let script = format!(
        "read_verilog {}; proc; techmap -map {}; clean -purge; write_verilog -noattr {}",
        quote(input),
        quote(techmap),
        quote(output),
    );
    capture(&["-q"], &script).map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_special_characters() {
        assert_eq!(quote(Path::new("a b.v")), "\"a b.v\"");
        assert_eq!(quote(Path::new("a\"b")), "\"a\\\"b\"");
    }

    #[test]
    fn rename_script_repeats() {
        let s = rename_script(Path::new("x.v"), 3);
        assert_eq!(s.matches("rename -hide").count(), 3);
    }

    #[test]
    fn parses_rename_seconds() {
        let report = "Time spent: 50% 3x foo (0 sec)\n  1.5 sec rename";
        assert_eq!(rename_seconds(report), Some(1.5));
        assert_eq!(rename_seconds("no timings"), None);
    }
}
