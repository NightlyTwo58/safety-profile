/// Core pipeline library for safety-profiler.
///
/// This is the single source of truth for pipeline execution.  Every consumer
/// — the `safety-profiler` CLI, the `compare` harness, and the Criterion
/// benches — calls into here rather than duplicating stage logic.
///
/// # Stage sequence
///
/// ```text
/// Verilog source str
///     │
///     ▼ parse      sv_parser::parse_sv_str
///     │
///     ▼ compile    nl_compiler::from_vast  (first/top module)
///     │
///     ▼ clean1     safety_pass::Clean
///     │
///     ▼ fold       safety_pass::FoldAllPatterns
///     │
///     ▼ clean2     safety_pass::Clean
///     │
///     ▼ emit       safety_pass::PrintVerilog
///     │
///     ▼ String (Verilog output)
/// ```
use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog};
use safety_pass::{Cell, Pass};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::time::{Duration, Instant};
use sv_parser::parse_sv_str;

//  Public types 

/// Timing and optional diagnostic message for one pipeline stage.
#[derive(Debug, Clone)]
pub struct StageTiming {
    /// Short label, e.g. `"parse"`, `"clean1"`.  Never changes between runs.
    pub name: &'static str,
    pub elapsed: Duration,
    /// Human-readable note emitted by the pass, e.g. `"removed 4 dead cells"`.
    pub note: Option<String>,
}

impl StageTiming {
    #[inline]
    pub fn micros(&self) -> u128 {
        self.elapsed.as_micros()
    }
}

/// The complete result of one pipeline execution.
#[derive(Debug)]
pub struct PipelineResult {
    /// One entry per stage, in execution order.
    pub stages: Vec<StageTiming>,
    /// Emitted Verilog from the final `PrintVerilog` pass.
    pub verilog: String,
}

impl PipelineResult {
    /// Sum of all stage timings in microseconds.
    pub fn total_micros(&self) -> u128 {
        self.stages.iter().map(|s| s.micros()).sum()
    }

    /// Look up a stage by name.  Panics if the name is not found — stages are
    /// static, so a missing name is a programming error, not a runtime one.
    pub fn stage(&self, name: &str) -> &StageTiming {
        self.stages
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no stage named {name:?}"))
    }
}

//  Error type 

/// Thin wrapper so callers do not need to import `Box<dyn Error>` everywhere.
pub type PipelineError = Box<dyn std::error::Error + Send + Sync + 'static>;

//  Public helpers the benches use for per-stage isolation 
//
// These expose the individual stages so Criterion can set up state in a setup
// closure (not timed) and only measure the stage under test.

/// Stage 1: Parse Verilog source into an AST.
pub fn stage_parse(
    src: &str,
    path: &Path,
) -> Result<sv_parser::SyntaxTree, PipelineError> {
    let (ast, _) = parse_sv_str(
        src,
        path.to_path_buf(),
        &HashMap::new(),
        &[] as &[std::path::PathBuf],
        true,
        false,
    )?;
    Ok(ast)
}

/// Stage 2: Compile AST → Netlist (first/top module).
pub fn stage_compile(
    ast: &sv_parser::SyntaxTree,
) -> Result<std::rc::Rc<safety_net::Netlist<Cell>>, PipelineError> {
    let netlist = from_vast::<Cell>(ast)?
        .into_iter()
        .next()
        .ok_or("no modules found in file")?;
    Ok(netlist)
}

/// Stage 3 / 5: Clean pass (dead-cell and dead-wire removal).
pub fn stage_clean(
    netlist: &std::rc::Rc<safety_net::Netlist<Cell>>,
) -> Result<String, PipelineError> {
    Ok(Clean(PhantomData::<Cell>).run(netlist)?)
}

/// Stage 4: Fold constant and pattern-matchable expressions.
pub fn stage_fold(
    netlist: &std::rc::Rc<safety_net::Netlist<Cell>>,
) -> Result<String, PipelineError> {
    Ok(FoldAllPatterns.run(netlist)?)
}

/// Stage 6: Emit Verilog from the optimised netlist.
pub fn stage_emit(
    netlist: &std::rc::Rc<safety_net::Netlist<Cell>>,
) -> Result<String, PipelineError> {
    Ok(PrintVerilog(PhantomData::<Cell>).run(netlist)?)
}

//  Full pipeline 

/// Run the complete pipeline and return structured timing + output.
///
/// File I/O is the caller's responsibility — pass the already-loaded source
/// string and the path (used by the parser for diagnostics only).
pub fn run_pipeline(src: &str, path: &Path) -> Result<PipelineResult, PipelineError> {
    let mut stages: Vec<StageTiming> = Vec::with_capacity(6);

    macro_rules! timed {
        ($name:literal, $expr:expr) => {{
            let t = Instant::now();
            let result = $expr?;
            stages.push(StageTiming {
                name: $name,
                elapsed: t.elapsed(),
                note: None,
            });
            result
        }};
        ($name:literal, note, $expr:expr) => {{
            let t = Instant::now();
            let (result, note) = $expr?;
            stages.push(StageTiming {
                name: $name,
                elapsed: t.elapsed(),
                note: Some(note),
            });
            result
        }};
    }

    let ast = timed!("parse", stage_parse(src, path).map(|v| v));

    let netlist = timed!("compile", stage_compile(&ast).map(|v| v));

    let _ = {
        let t = Instant::now();
        let note = stage_clean(&netlist)?;
        stages.push(StageTiming { name: "clean1", elapsed: t.elapsed(), note: Some(note) });
    };

    let _ = {
        let t = Instant::now();
        let note = stage_fold(&netlist)?;
        stages.push(StageTiming { name: "fold", elapsed: t.elapsed(), note: Some(note) });
    };

    let _ = {
        let t = Instant::now();
        let note = stage_clean(&netlist)?;
        stages.push(StageTiming { name: "clean2", elapsed: t.elapsed(), note: Some(note) });
    };

    let verilog = {
        let t = Instant::now();
        let v = stage_emit(&netlist)?;
        stages.push(StageTiming { name: "emit", elapsed: t.elapsed(), note: None });
        v
    };

    Ok(PipelineResult { stages, verilog })
}
