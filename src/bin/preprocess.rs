/// ISCAS-85 Verilog preprocessor.
///
/// Usage:
///   cargo run --bin preprocess --release -- [raw_dir] [out_dir]
///   cargo run --bin preprocess --release           # defaults: inputs/raw → inputs/
///
/// # Why this is needed
///
/// Canonical ISCAS-85 files use Verilog built-in **gate primitive** syntax:
///
///     nand (N10, N1, N3);
///
/// `nl_compiler::from_vast` does not handle gate primitives — it processes
/// **module instantiations** and calls `Cell::from_id` on the instantiation
/// type name.  `Cell::from_id` delegates to `CellType::from_str`, which
/// expects uppercase names matching the `CellType` enum variants.
///
/// Additionally, gate primitives cannot have instance names, but every node
/// in the netlist needs an identity.
///
/// # Transformation applied (per file)
///
/// For each gate primitive line:
///
///     nand (N10, N1, N3);               ← gate primitive, lowercase, anonymous
///
/// We emit a module instantiation:
///
///     NAND2 g_N10 (.A(N1), .B(N3), .Y(N10));
///
/// Specifically:
///   1. Uppercase the gate type and select the sized variant where one exists
///      (`NAND2` for a 2-input nand, `NAND3` for 3-input, etc.).  Falls back
///      to the generic variant (`NAND`) when no sized variant is defined.
///   2. Synthesise a unique instance name from the output net: `g_<out>`.
///   3. Reorder ports to named connections: output first arg → `.Y()`,
///      remaining args → `.A()`, `.B()`, `.C()`, `.D()` in order.
///   4. Add explicit `wire` declarations for all internal nets (those not in
///      the module port list).
///   5. Strip `` `timescale `` directives.
///
/// Port naming follows the `safety_net::Gate` convention visible in the docs:
///   inputs: A, B, C, D  |  output: Y
///
/// Yosys is unaffected — it can read either form — so both pipelines run from
/// the same preprocessed file.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::PathBuf,
};

//  Gate primitive → CellType mapping 

/// Maps a lowercase Verilog gate primitive name and input count to the
/// uppercase `CellType` name that `Cell::from_id` / `CellType::from_str`
/// will accept.
///
/// Sized variants (`NAND2`, `NAND3`, `NAND4`) are preferred where they exist
/// in the `CellType` enum.  Falls back to the generic variant otherwise.
fn cell_type_name(primitive: &str, n_inputs: usize) -> String {
    match primitive {
        "nand" => match n_inputs {
            2 => "NAND2",
            3 => "NAND3",
            4 => "NAND4",
            _ => "NAND",
        },
        "and" => match n_inputs {
            2 => "AND2",
            3 => "AND3",
            4 => "AND4",
            _ => "AND",
        },
        "nor" => match n_inputs {
            2 => "NOR2",
            3 => "NOR3",
            4 => "NOR4",
            _ => "NOR",
        },
        "or" => match n_inputs {
            2 => "OR2",
            3 => "OR3",
            4 => "OR4",
            _ => "OR",
        },
        "xor"  => match n_inputs { 2 => "XOR2",  _ => "XOR" },
        "xnor" => match n_inputs { 2 => "XNOR2", _ => "XNOR" },
        // NOT and INV are both in CellType; map Verilog `not` → NOT.
        "not"  => "NOT",
        // BUF is a single-input buffer.
        "buf"  => "BUF",
        // Unknown primitive: pass through uppercased and hope for the best.
        other  => return other.to_uppercase(),
    }
    .to_string()
}

/// Input port labels A, B, C, D, ...
/// Panics if n > 26, which no real gate hits.
fn input_label(index: usize) -> char {
    (b'A' + index as u8) as char
}

//  Gate primitive detection 

const PRIMITIVES: &[&str] = &[
    "nand", "and", "nor", "or", "xor", "xnor", "not", "buf",
];

fn is_primitive_keyword(word: &str) -> bool {
    PRIMITIVES.contains(&word)
}

//  Per-file transformation 

struct ProcessedFile {
    content: String,
    /// Lines converted from gate primitive to module instantiation.
    converted: usize,
    /// Wire declarations inserted.
    wires_added: usize,
}

/// Extract port names from `module foo (A, B, Y);` → {"A", "B", "Y"}.
fn extract_ports(line: &str) -> HashSet<String> {
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
    // `declared` tracks ports + explicitly declared wires so we don't re-emit
    // a wire decl for them.  We reuse the ports set for simplicity.
    let mut declared: HashSet<String> = HashSet::new();
    // Nets driven by converted gate outputs that may need wire declarations.
    let mut driven: BTreeSet<String> = BTreeSet::new();
    let mut lines_out: Vec<String> = Vec::with_capacity(src.lines().count() + 8);
    let mut converted = 0usize;

    for line in src.lines() {
        let trimmed = line.trim();
        let indent = &line[..line.len() - trimmed.len()];

        //  Step 1: strip `timescale 
        if trimmed.starts_with("`timescale") {
            continue;
        }

        //  Collect port names 
        if trimmed.starts_with("module ") {
            declared = extract_ports(trimmed);
            lines_out.push(line.to_string());
            continue;
        }

        //  Track explicit wire declarations 
        if trimmed.starts_with("wire ") || trimmed.starts_with("wire\t") {
            let decl = trimmed
                .strip_prefix("wire")
                .unwrap_or("")
                .trim()
                .trim_end_matches(';');
            for net in decl.split(',') {
                declared.insert(net.trim().to_string());
            }
            lines_out.push(line.to_string());
            continue;
        }

        //  Step 2: detect and convert gate primitives 
        //
        // Gate primitive syntax:  <keyword> ( <out>, <in0>, <in1>, ... );
        // Module instantiation:   <TYPE> <name> ( .<port>(<net>), ... );
        //
        // Key distinction from a named module instance:
        //   gate primitive  →  keyword immediately followed by '('
        //   module inst     →  keyword, then whitespace, then an identifier
        //
        let mut matched = false;
        for &prim in PRIMITIVES {
            let rest = match trimmed.strip_prefix(prim) {
                Some(r) => r,
                None => continue,
            };
            // Must be followed by optional whitespace then '(' to be a primitive.
            let rest_trimmed = rest.trim_start();
            if !rest_trimmed.starts_with('(') {
                // Has an instance name already (named module inst) — pass through.
                // Still record the driven net for wire-decl purposes.
                if let Some(a) = rest_trimmed.find('(') {
                    let args = &rest_trimmed[a + 1..rest_trimmed.rfind(')').unwrap_or(rest_trimmed.len())];
                    if let Some(out) = args.split(',').next() {
                        driven.insert(out.trim().to_string());
                    }
                }
                lines_out.push(line.to_string());
                matched = true;
                break;
            }

            // Anonymous gate primitive — convert it.
            let args_str = &rest_trimmed[1..rest_trimmed.rfind(')').unwrap_or(rest_trimmed.len())];
            let args: Vec<&str> = args_str.split(',').map(str::trim).collect();

            if args.is_empty() {
                lines_out.push(line.to_string());
                matched = true;
                break;
            }

            // In Verilog gate primitives, the first argument is the OUTPUT.
            let out_net = args[0];
            let in_nets = &args[1..];
            let n_inputs = in_nets.len();

            let cell_name = cell_type_name(prim, n_inputs);
            let inst_name = format!("g_{out_net}");

            // Build named port connection list:  .A(in0), .B(in1), .Y(out)
            let mut port_list = String::new();
            for (i, &net) in in_nets.iter().enumerate() {
                if i > 0 { port_list.push_str(", "); }
                port_list.push('.');
                port_list.push(input_label(i));
                port_list.push('(');
                port_list.push_str(net);
                port_list.push(')');
            }
            if n_inputs > 0 { port_list.push_str(", "); }
            port_list.push_str(".Y(");
            port_list.push_str(out_net);
            port_list.push(')');

            lines_out.push(format!("{indent}{cell_name} {inst_name} ({port_list});"));
            driven.insert(out_net.to_string());
            converted += 1;
            matched = true;
            break;
        }

        if !matched {
            lines_out.push(line.to_string());
        }
    }

    //  Step 3: splice wire declarations before `endmodule` 
    let undeclared: Vec<&str> = driven
        .iter()
        .filter(|n| !declared.contains(*n))
        .map(|n| n.as_str())
        .collect();
    let wires_added = undeclared.len();

    let mut out = String::with_capacity(src.len() + 256);
    for line in &lines_out {
        if line.trim() == "endmodule" && !undeclared.is_empty() {
            out.push_str(&format!("    wire {};\n", undeclared.join(", ")));
        }
        out.push_str(line);
        out.push('\n');
    }

    ProcessedFile { content: out, converted, wires_added }
}

//  Entry point 

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let raw_dir = PathBuf::from(args.get(1).map(String::as_str).unwrap_or("inputs/raw"));
    let out_dir = PathBuf::from(args.get(2).map(String::as_str).unwrap_or("inputs"));

    if !raw_dir.exists() {
        eprintln!(
            "ERROR: '{}' not found.\nPlace unmodified ISCAS-85 .v files there and re-run.",
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
                "  {name}: OK  ({} converted, {} wires added) → {}",
                result.converted, result.wires_added, dst.display()
            ),
            Err(e) => { eprintln!("  {name}: write error: {e}"); failures += 1; }
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

//  Tests 

#[cfg(test)]
mod tests {
    use super::*;

    const C17_RAW: &str = "\
module c17 (N1, N2, N3, N6, N7, N22, N23);
input N1, N2, N3, N6, N7;
output N22, N23;
nand (N10, N1, N3);
nand (N11, N3, N6);
nand (N16, N2, N11);
nand (N19, N11, N7);
nand (N22, N10, N16);
nand (N23, N16, N19);
endmodule
";

    #[test]
    fn gate_primitives_become_module_instantiations() {
        let out = process(C17_RAW);
        // Must be module instantiation syntax, not gate primitive.
        assert!(out.content.contains("NAND2 g_N10 (.A(N1), .B(N3), .Y(N10));"));
        assert!(out.content.contains("NAND2 g_N22 (.A(N10), .B(N16), .Y(N22));"));
        assert_eq!(out.converted, 6);
    }

    #[test]
    fn no_gate_primitive_syntax_remains() {
        let out = process(C17_RAW);
        // The old form must not survive.
        assert!(!out.content.contains("nand ("));
    }

    #[test]
    fn internal_wires_declared() {
        let out = process(C17_RAW);
        // N10, N11, N16, N19 are internal nets.
        assert!(out.content.contains("wire"));
        assert!(out.content.contains("N10"));
        // Output ports N22, N23 must NOT be re-declared.
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
    fn sized_variants_by_input_count() {
        assert_eq!(cell_type_name("nand", 2), "NAND2");
        assert_eq!(cell_type_name("nand", 3), "NAND3");
        assert_eq!(cell_type_name("nand", 4), "NAND4");
        assert_eq!(cell_type_name("nand", 5), "NAND");   // fallback
        assert_eq!(cell_type_name("not",  1), "NOT");
        assert_eq!(cell_type_name("buf",  1), "BUF");
        assert_eq!(cell_type_name("xor",  2), "XOR2");
    }

    #[test]
    fn three_input_gate() {
        let src = "module m(a,b,c,y);\ninput a,b,c;\noutput y;\nnand (y, a, b, c);\nendmodule\n";
        let out = process(src);
        assert!(out.content.contains("NAND3 g_y (.A(a), .B(b), .C(c), .Y(y));"));
    }

    #[test]
    fn already_named_instance_passes_through() {
        // A module instantiation that already has a name should be unchanged.
        let src = "module m(a,b,y);\ninput a,b;\noutput y;\nNAND2 g1 (.A(a), .B(b), .Y(y));\nendmodule\n";
        let out = process(src);
        assert!(out.content.contains("NAND2 g1 (.A(a), .B(b), .Y(y));"));
        assert_eq!(out.converted, 0);
    }
}