//! Snapshot regression tests.
//!
//! Pipeline and rename output for every circuit in `inputs/` must match
//! `tests/golden/{pipeline,renamed}/<circuit>.v`. Update snapshots after an
//! intended change with `BLESS=1 cargo test --test regression`.

use safety_profiler::{
    io::{collect_inputs, stem},
    rename_netlist, run_pipeline,
};
use std::{env, fs, path::PathBuf};

/// Circuits the pipeline does not support (`X` constants).
const SKIP: &[&str] = &["gnd"];

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Compare `actual` to its snapshot, or rewrite the snapshot under `BLESS`.
fn check(kind: &str, name: &str, actual: &str) -> Option<String> {
    let path = root()
        .join("tests/golden")
        .join(kind)
        .join(format!("{name}.v"));
    if env::var_os("BLESS").is_some() {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, actual).unwrap();
        return None;
    }
    match fs::read_to_string(&path) {
        Ok(expected) if expected == actual => None,
        Ok(_) => Some(format!(
            "{name}: {kind} output differs from {}",
            path.display()
        )),
        Err(_) => Some(format!(
            "{name}: no snapshot at {} (run with BLESS=1)",
            path.display()
        )),
    }
}

#[test]
fn outputs_match_snapshots() {
    let inputs = collect_inputs(&[root().join("inputs")], None).unwrap();
    let mut failures = Vec::new();
    for path in inputs {
        let name = stem(&path);
        if SKIP.contains(&name.as_str()) {
            continue;
        }
        let src = fs::read_to_string(&path).unwrap();
        match run_pipeline(&src, &path) {
            Ok(r) => failures.extend(check("pipeline", &name, &r.verilog)),
            Err(e) => failures.push(format!("{name}: pipeline failed: {e}")),
        }
        match rename_netlist(&src, &path) {
            Ok(v) => failures.extend(check("renamed", &name, &v)),
            Err(e) => failures.push(format!("{name}: rename failed: {e}")),
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
