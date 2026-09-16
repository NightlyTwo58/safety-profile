use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog};
use safety_pass::{Cell, Pass};
use std::marker::PhantomData;
use std::{collections::HashMap, env, fs, path::PathBuf, time::Instant};
use sv_parser::parse_sv_str;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: safety-profiler <input.v>");
        std::process::exit(1);
    }

    let path = PathBuf::from(&args[1]);
    let src = fs::read_to_string(&path).expect("Failed to read input file");

    //  Stage 1: Parse 
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
    println!("[parse]    {:>10.3?}", t.elapsed());

    //  Stage 2: Compile to Netlist(s) 
    // from_vast returns a Vec — one Netlist per module in the file.
    // For benchmarking we take the first (top) module.
    let t = Instant::now();
    let netlists = from_vast::<Cell>(&ast).expect("Failed to compile to netlist");
    let netlist = netlists.into_iter().next().expect("No modules found in file");
    println!("[compile]  {:>10.3?}", t.elapsed());

    //  Stage 3: Clean 
    let t = Instant::now();
    let msg = Clean(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Clean failed");
    println!("[clean]    {:>10.3?}  {}", t.elapsed(), msg);

    //  Stage 4: Fold all patterns 
    let t = Instant::now();
    let msg = FoldAllPatterns.run(&netlist).expect("Fold failed");
    println!("[fold]     {:>10.3?}  {}", t.elapsed(), msg);

    //  Stage 5: Clean again 
    let t = Instant::now();
    let msg = Clean(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Clean 2 failed");
    println!("[clean2]   {:>10.3?}  {}", t.elapsed(), msg);

    //  Stage 6: Emit 
    let t = Instant::now();
    let verilog = PrintVerilog(PhantomData::<Cell>)
        .run(&netlist)
        .expect("Emit failed");
    println!("[emit]     {:>10.3?}", t.elapsed());

    // Write output next to input
    let out_path = path.with_extension("out.v");
    fs::write(&out_path, verilog).expect("Failed to write output");
    println!("Written to {}", out_path.display());
}