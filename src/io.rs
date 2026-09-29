/// Input discovery and output layout shared by every binary.
///
/// Output layout (root defaults to `output/`, never under `target/`):
///
/// ```text
/// output/
///   stats/        CSV files
///   flamegraphs/  one SVG per circuit
///   verilog/ours/ and verilog/yosys/
/// ```
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// True for files that are valid pipeline inputs: `*.v`, excluding generated
/// artifacts and the techmap.
fn is_input(p: &Path) -> bool {
    let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
    p.extension().is_some_and(|e| e == "v")
        && !name.ends_with(".out.v")
        && !name.ends_with(".normalized.v")
        && name != "techmap.v"
}

/// Resolve CLI paths into a sorted, de-duplicated list of Verilog files.
///
/// * a file is taken as given (even if it would be filtered in a directory)
/// * a directory is expanded (non-recursively) to its `*.v` files
/// * `filter`, if set, keeps only files whose stem contains the substring
pub fn collect_inputs(paths: &[PathBuf], filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for p in paths {
        if p.is_dir() {
            let rd = fs::read_dir(p).map_err(|e| format!("{}: {e}", p.display()))?;
            out.extend(rd.filter_map(|e| e.ok().map(|e| e.path())).filter(|p| is_input(p)));
        } else if p.is_file() {
            out.push(p.clone());
        } else {
            return Err(format!("{}: no such file or directory", p.display()));
        }
    }
    if let Some(f) = filter {
        out.retain(|p| p.file_stem().is_some_and(|s| s.to_string_lossy().contains(f)));
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err("no matching .v files found".into());
    }
    Ok(out)
}

/// Root of all generated artifacts.  Subdirectories are created on demand.
pub struct OutDir(PathBuf);

impl OutDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(root.into())
    }

    fn sub(&self, parts: &[&str]) -> io::Result<PathBuf> {
        let p = parts.iter().fold(self.0.clone(), |acc, s| acc.join(s));
        fs::create_dir_all(&p)?;
        Ok(p)
    }

    pub fn stats(&self) -> io::Result<PathBuf> {
        self.sub(&["stats"])
    }

    pub fn flamegraphs(&self) -> io::Result<PathBuf> {
        self.sub(&["flamegraphs"])
    }

    /// `tool` is `"ours"` or `"yosys"`.
    pub fn verilog(&self, tool: &str) -> io::Result<PathBuf> {
        self.sub(&["verilog", tool])
    }
}
