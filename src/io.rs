//! Input discovery and output layout shared by every binary.
//!
//! Output layout (root defaults to `output/`; directories are created on demand):
//!
//! ```text
//! output/
//!   stats/          compare CSV files
//!   flamegraphs/    <circuit>.pipeline.svg
//!   verilog/ours/   emitted netlists from this pipeline
//!   verilog/yosys/  emitted netlists from Yosys
//! ```

use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// File stem of `path` as an owned string.
pub fn stem(path: &Path) -> String {
    path.file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .into_owned()
}

/// True for valid pipeline inputs: `*.v`, excluding generated files and `techmap.v`.
fn is_input(path: &Path) -> bool {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    path.extension().is_some_and(|e| e == "v")
        && !name.ends_with(".out.v")
        && !name.ends_with(".normalized.v")
        && name != "techmap.v"
}

/// Resolve CLI paths into a sorted, de-duplicated list of Verilog files.
///
/// Files are taken as given. Directories are expanded non-recursively to their
/// inputs. `filter` keeps only files whose stem contains the substring.
pub fn collect_inputs(paths: &[PathBuf], filter: Option<&str>) -> Result<Vec<PathBuf>, String> {
    let mut out = Vec::new();
    for path in paths {
        if path.is_dir() {
            let entries = fs::read_dir(path).map_err(|e| format!("{}: {e}", path.display()))?;
            out.extend(
                entries
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .filter(|p| is_input(p)),
            );
        } else if path.is_file() {
            out.push(path.clone());
        } else {
            return Err(format!("{}: no such file or directory", path.display()));
        }
    }
    if let Some(filter) = filter {
        out.retain(|p| stem(p).contains(filter));
    }
    out.sort();
    out.dedup();
    if out.is_empty() {
        return Err("no matching .v files found".into());
    }
    Ok(out)
}

/// Remove inputs whose stem contains any of `excludes`; returns how many were removed.
pub fn exclude_inputs(inputs: &mut Vec<PathBuf>, excludes: &[String]) -> usize {
    let before = inputs.len();
    inputs.retain(|p| {
        let stem = stem(p);
        !excludes.iter().any(|x| stem.contains(x.as_str()))
    });
    before - inputs.len()
}

/// Root of all generated artifacts.
pub struct OutDir(PathBuf);

impl OutDir {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self(root.into())
    }

    fn sub(&self, parts: &[&str]) -> io::Result<PathBuf> {
        let path = parts.iter().fold(self.0.clone(), |acc, s| acc.join(s));
        fs::create_dir_all(&path)?;
        Ok(path)
    }

    pub fn stats(&self) -> io::Result<PathBuf> {
        self.sub(&["stats"])
    }

    pub fn flamegraphs(&self) -> io::Result<PathBuf> {
        self.sub(&["flamegraphs"])
    }

    /// Path of the pipeline flamegraph for the circuit named `stem`.
    pub fn pipeline_flamegraph(&self, stem: &str) -> io::Result<PathBuf> {
        Ok(self.flamegraphs()?.join(format!("{stem}.pipeline.svg")))
    }

    /// Verilog directory for `tool`, either `"ours"` or `"yosys"`.
    pub fn verilog(&self, tool: &str) -> io::Result<PathBuf> {
        self.sub(&["verilog", tool])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skips_generated_files() {
        assert!(is_input(Path::new("c17.v")));
        assert!(!is_input(Path::new("c17.out.v")));
        assert!(!is_input(Path::new("c17.normalized.v")));
        assert!(!is_input(Path::new("techmap.v")));
        assert!(!is_input(Path::new("c17.txt")));
    }

    #[test]
    fn exclude_counts_removed() {
        let mut v = vec![PathBuf::from("a/c17.v"), PathBuf::from("a/gnd.v")];
        assert_eq!(exclude_inputs(&mut v, &["gnd".to_string()]), 1);
        assert_eq!(v, [PathBuf::from("a/c17.v")]);
    }
}
