/// ISCAS-85 Verilog preprocessor.
///
/// Usage:
///   cargo run --bin preprocess --release -- [raw_dir] [out_dir]
///   cargo run --bin preprocess --release           # defaults: inputs/raw → inputs/
///
/// Converts canonical ISCAS-85 gate-level Verilog into the subset accepted by
/// both Yosys and Matt's nl_compiler.
///
/// # Why this is needed
///
/// Canonical ISCAS-85 files use anonymous Verilog gate primitives:
///
///     nand (N10, N1, N3);
///
/// Yosys accepts these.  nl_compiler requires named instances because it
/// builds a netlist graph where every cell needs a stable identity:
///
///     nand g_N10 (N10, N1, N3);
///
/// Additional issues in some distributions:
///   - Missing `wire` declarations for internal nets
///   - `` `timescale `` directives (harmless but noisy for our parser)
///
/// # Transformations applied (in order, per file)
///
///   1. Strip `` `timescale `` lines
///   2. Detect anonymous primitive instances and add instance names
///   3. Collect all driven nets and insert `wire` declarations for any
///      that are not in the module's port list
///
/// Both pipelines run on the SAME preprocessed file.  The preprocessing cost
/// is a one-time setup step and is NOT counted in any benchmark.
use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::{PathBuf},
};

//  Gate primitives we handle 

const PRIMITIVES: &[&str] = &["and", "nand", "or", "nor", "xor", "xnor", "not", "buf"];

//  Per-file transformation 

struct ProcessedFile {
    content: String,
    /// Number of anonymous instances that were named.
    renamed: usize,
    /// Number of wire declarations inserted.
    wires_added: usize,
}

/// Parse the port list from a `module ... (ports);` line.
/// Returns a set of port names.  We only need this to avoid re-declaring them
/// as `wire`.
fn extract_ports(line: &str) -> HashSet<String> {
    // Everything inside the outermost parentheses
    let inner = match (line.find('('), line.rfind(')')) {
        (Some(a), Some(b)) if b > a => &line[a + 1..b],
        _ => return HashSet::new(),
    };
    inner
        .split(',')
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect()
}

fn process(src: &str) -> ProcessedFile {
    let mut out = String::with_capacity(src.len() + 256);
    let mut ports: HashSet<String> = HashSet::new();
    // Nets driven by gate outputs that need wire declarations.
    // BTreeSet so the emitted `wire` line is deterministic / readable.
    let mut driven_nets: BTreeSet<String> = BTreeSet::new();
    let mut renamed = 0usize;
    let mut wires_added = 0usize;

    // We do two passes:
    //   Pass 1 — build the transformed line list (steps 1 & 2)
    //   Pass 2 — insert wire declarations just before `endmodule`
    // We store pass-1 output in a Vec<String> to splice in the wires.

    let mut lines_out: Vec<String> = Vec::with_capacity(src.lines().count() + 8);

    for line in src.lines() {
        let trimmed = line.trim();

        //  Step 1: strip `timescale 
        if trimmed.starts_with("`timescale") {
            continue;
        }

        //  Collect port names from module declaration 
        if trimmed.starts_with("module ") {
            ports = extract_ports(trimmed);
            lines_out.push(line.to_string());
            continue;
        }

        //  Collect explicitly declared wires (don't re-declare) 
        if trimmed.starts_with("wire ") {
            // Mark these nets as already declared.
            let decl = trimmed
                .trim_start_matches("wire ")
                .trim_end_matches(';')
                .trim();
            for net in decl.split(',') {
                ports.insert(net.trim().to_string()); // reuse `ports` as "already declared"
            }
            lines_out.push(line.to_string());
            continue;
        }

        //  Step 2: detect and name anonymous gate primitives 
        //
        // A named instance:    nand g1 (out, in1, in2);
        // An anonymous one:    nand    (out, in1, in2);
        //
        // Heuristic: after the gate keyword, the very next non-whitespace
        // character is '(' → anonymous.
        let mut found_primitive = false;
        for &prim in PRIMITIVES {
            // Must start with the primitive name followed by whitespace or '('
            if let Some(rest) = trimmed.strip_prefix(prim) {
                let rest = rest.trim_start();
                if rest.starts_with('(') {
                    // Anonymous — extract output net (first arg) and add name.
                    let args_inner = match rest.find('(').and_then(|a| rest.rfind(')').map(|b| (a, b))) {
                        Some((a, b)) => &rest[a + 1..b],
                        None => { lines_out.push(line.to_string()); found_primitive = true; break; }
                    };

                    let output_net = args_inner
                        .split(',')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .to_string();

                    if !output_net.is_empty() {
                        // Synthesise an instance name from the output net.
                        let inst_name = format!("g_{output_net}");
                        // Preserve original indentation.
                        let indent = &line[..line.len() - line.trim_start().len()];
                        lines_out.push(format!(
                            "{indent}{prim} {inst_name} ({args_inner});"
                        ));
                        driven_nets.insert(output_net);
                        renamed += 1;
                    } else {
                        lines_out.push(line.to_string());
                    }
                    found_primitive = true;
                    break;
                } else {
                    // Already has an instance name — still record the output net.
                    // args start after the instance name token.
                    if let Some(a) = rest.find('(') {
                        let args_inner = &rest[a + 1..rest.rfind(')').unwrap_or(rest.len())];
                        if let Some(out_net) = args_inner.split(',').next() {
                            driven_nets.insert(out_net.trim().to_string());
                        }
                    }
                    lines_out.push(line.to_string());
                    found_primitive = true;
                    break;
                }
            }
        }

        if !found_primitive {
            lines_out.push(line.to_string());
        }
    }

    //  Step 3: insert wire declarations before `endmodule` 
    let undeclared: Vec<&str> = driven_nets
        .iter()
        .filter(|n| !ports.contains(*n))
        .map(|n| n.as_str())
        .collect();

    for line in &lines_out {
        if line.trim() == "endmodule" && !undeclared.is_empty() {
            let wire_decl = format!("    wire {};\n", undeclared.join(", "));
            out.push_str(&wire_decl);
            wires_added = undeclared.len();
        }
        out.push_str(line);
        out.push('\n');
    }

    ProcessedFile { content: out, renamed, wires_added }
}

//  Entry point 

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw_dir = PathBuf::from(args.get(1).map(|s| s.as_str()).unwrap_or("inputs/raw"));
    let out_dir = PathBuf::from(args.get(2).map(|s| s.as_str()).unwrap_or("inputs"));

    if !raw_dir.exists() {
        eprintln!(
            "ERROR: raw input directory '{}' not found.\n\
             Place unmodified ISCAS-85 .v files there and re-run.",
            raw_dir.display()
        );
        std::process::exit(1);
    }

    fs::create_dir_all(&out_dir).expect("Failed to create output directory");

    println!("=== Preprocessing ISCAS-85 inputs ===");
    println!("  raw:    {}", raw_dir.display());
    println!("  output: {}\n", out_dir.display());

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
        let src = match fs::read_to_string(input) {
            Ok(s) => s,
            Err(e) => { eprintln!("  {name}: read error: {e}"); failures += 1; continue; }
        };

        let result = process(&src);

        let dst = out_dir.join(input.file_name().unwrap());
        match fs::write(&dst, &result.content) {
            Ok(_) => println!(
                "  {name}: OK  ({} renamed, {} wires added) → {}",
                result.renamed, result.wires_added, dst.display()
            ),
            Err(e) => { eprintln!("  {name}: write error: {e}"); failures += 1; }
        }
    }

    println!();
    if failures == 0 {
        println!("All inputs preprocessed successfully.");
        println!("Run:  cargo run --bin compare --release");
    } else {
        eprintln!("{failures} file(s) failed.");
        std::process::exit(1);
    }
}

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;

    const C17_RAW: &str = r#"module c17 (N1, N2, N3, N6, N7, N22, N23);
input N1, N2, N3, N6, N7;
output N22, N23;
nand (N10, N1, N3);
nand (N11, N3, N6);
nand (N16, N2, N11);
nand (N19, N11, N7);
nand (N22, N10, N16);
nand (N23, N16, N19);
endmodule
"#;

    #[test]
    fn anonymous_instances_are_named() {
        let out = process(C17_RAW);
        assert!(out.content.contains("nand g_N10 (N10, N1, N3);"));
        assert!(out.content.contains("nand g_N22 (N22, N10, N16);"));
        assert_eq!(out.renamed, 6);
    }

    #[test]
    fn internal_wires_declared() {
        let out = process(C17_RAW);
        // N10, N11, N16, N19 are internal; N22, N23 are ports (outputs)
        assert!(out.content.contains("wire"));
        assert!(out.content.contains("N10"));
        // Output ports should NOT be re-declared as wires
        assert!(!out.content.contains("wire N22"));
        assert!(!out.content.contains("wire N23"));
    }

    #[test]
    fn timescale_stripped() {
        let src = "`timescale 1ns/1ps\n".to_string() + C17_RAW;
        let out = process(&src);
        assert!(!out.content.contains("`timescale"));
    }

    #[test]
    fn already_named_instances_pass_through() {
        let src = "module m (a, b, y);\ninput a, b;\noutput y;\nnand g1 (y, a, b);\nendmodule\n";
        let out = process(src);
        assert!(out.content.contains("nand g1 (y, a, b);"));
        assert_eq!(out.renamed, 0);
    }
}
