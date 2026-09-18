use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog};
use safety_pass::{Cell, Pass};
use std::marker::PhantomData;
use std::{collections::HashMap, env, fs, path::PathBuf, time::Instant};
use sv_parser::parse_sv_str;

macro_rules! stage {
    ($label:expr, $elapsed:expr) => {
        println!("[{}]  {}µs", $label, $elapsed.as_micros());
    };
    ($label:expr, $elapsed:expr, $msg:expr) => {
        println!("[{}]  {}µs  {}", $label, $elapsed.as_micros(), $msg);
    };
}

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: safety-profiler <input.v>");
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    let src = fs::read_to_string(&path).expect("Failed to read input file");

    //  Parse 
    let t = Instant::now();
    let (ast, _) = parse_sv_str(
        &src,
        path.clone(),
        &HashMap::new(),
        &[] as &[PathBuf],
        true,
        false,
    )
    .expect("Failed to parse Verilog");
    stage!("parse", t.elapsed());

    //  Stage 2: Compile AST to Netlist)
    // from_vast returns one Netlist per module; we take the first (top) module.
    let t = Instant::now();
    let netlists = from_vast::<Cell>(&ast).expect("Failed to compile to netlist");
    let netlist = netlists.into_iter().next().expect("No modules found in file");
    stage!("compile", t.elapsed());

    //  Clean 1
    let t = Instant::now();
    let msg = Clean(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Clean pass 1 failed");
    stage!("clean1", t.elapsed(), msg);

    //  Fold all patterns 
    let t = Instant::now();
    let msg = FoldAllPatterns.run(&netlist).expect("Fold failed");
    stage!("fold", t.elapsed(), msg);

    //  Clean 2 
    let t = Instant::now();
    let msg = Clean(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Clean pass 2 failed");
    stage!("clean2", t.elapsed(), msg);

    //  Emit 
    let t = Instant::now();
    let verilog = PrintVerilog(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Emit failed");
    stage!("emit", t.elapsed());

    // Write output file
    let out_path = path.with_extension("out.v");
    fs::write(&out_path, verilog).expect("Failed to write output");
    eprintln!("Written to {}", out_path.display());
}
