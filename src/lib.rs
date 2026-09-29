/// Core pipeline library for safety-profiler.
///
/// This is the single source of truth for pipeline execution.  Every consumer
/// (the `safety-profiler` CLI, the `compare` harness, and the Criterion
/// benches) calls into here rather than duplicating stage logic.
///
/// # Stage sequence
///
/// ```text
/// Verilog source str
///     |
///     v parse      sv_parser::parse_sv_str
///     v compile    nl_compiler::from_vast  (first/top module)
///     v clean1     safety_pass::Clean
///     v fold       safety_pass::FoldAllPatterns
///     v clean2     safety_pass::Clean
///     v emit       safety_pass::PrintVerilog
///     v
/// String (Verilog output)
/// ```
pub mod flame;
pub mod io;
pub mod stats;

use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog};
use safety_pass::{Cell, Pass};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};
use sv_parser::parse_sv_str;

type Net = Rc<safety_net::Netlist<Cell>>;

/// Stage labels in execution order.  Shared by every consumer so column sets
/// cannot drift from what `run_pipeline` actually records.
pub const STAGE_NAMES: [&str; 6] = ["parse", "compile", "clean1", "fold", "clean2", "emit"];

// Public types

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
        self.stages.iter().map(StageTiming::micros).sum()
    }

    /// Look up a stage by name.  Panics if the name is not found: stages are
    /// static, so a missing name is a programming error, not a runtime one.
    pub fn stage(&self, name: &str) -> &StageTiming {
        self.stages
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no stage named {name:?}"))
    }
}

// Error type

/// Thin wrapper so callers do not need to import `Box<dyn Error>` everywhere.
pub type PipelineError = Box<dyn std::error::Error + Send + Sync + 'static>;

// Per-stage functions (used directly by the benches for isolation).
//
// `#[inline(never)]` keeps each stage as its own frame in flamegraphs.

/// Stage 1: Parse Verilog source into an AST.
#[inline(never)]
pub fn stage_parse(src: &str, path: &Path) -> Result<sv_parser::SyntaxTree, PipelineError> {
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

/// Stage 2: Compile AST -> Netlist (first/top module).
#[inline(never)]
pub fn stage_compile(ast: &sv_parser::SyntaxTree) -> Result<Net, PipelineError> {
    let netlist = from_vast::<Cell>(ast)?
        .into_iter()
        .next()
        .ok_or("no modules found in file")?;
    Ok(netlist)
}

/// Stage 3 / 5: Clean pass (dead-cell and dead-wire removal).
#[inline(never)]
pub fn stage_clean(netlist: &Net) -> Result<String, PipelineError> {
    Ok(Clean(PhantomData::<Cell>).run(netlist)?)
}

/// Stage 4: Fold constant and pattern-matchable expressions.
#[inline(never)]
pub fn stage_fold(netlist: &Net) -> Result<String, PipelineError> {
    Ok(FoldAllPatterns.run(netlist)?)
}

/// Stage 6: Emit Verilog from the optimised netlist.
#[inline(never)]
pub fn stage_emit(netlist: &Net) -> Result<String, PipelineError> {
    Ok(PrintVerilog(PhantomData::<Cell>).run(netlist)?)
}

// Full pipeline

/// Time `f`, record it under `name`, and return its value.
fn timed<T>(
    stages: &mut Vec<StageTiming>,
    name: &'static str,
    f: impl FnOnce() -> Result<T, PipelineError>,
) -> Result<T, PipelineError> {
    let t = Instant::now();
    let out = f()?;
    stages.push(StageTiming { name, elapsed: t.elapsed(), note: None });
    Ok(out)
}

/// Like `timed`, for passes whose returned `String` is a diagnostic note.
fn timed_note(
    stages: &mut Vec<StageTiming>,
    name: &'static str,
    f: impl FnOnce() -> Result<String, PipelineError>,
) -> Result<(), PipelineError> {
    let t = Instant::now();
    let note = f()?;
    stages.push(StageTiming { name, elapsed: t.elapsed(), note: Some(note) });
    Ok(())
}

/// Run the complete pipeline and return structured timing + output.
///
/// File I/O is the caller's responsibility: pass the already-loaded source
/// string and the path (used by the parser for diagnostics only).
pub fn run_pipeline(src: &str, path: &Path) -> Result<PipelineResult, PipelineError> {
    let mut stages = Vec::with_capacity(STAGE_NAMES.len());

    let ast = timed(&mut stages, "parse", || stage_parse(src, path))?;
    let netlist = timed(&mut stages, "compile", || stage_compile(&ast))?;
    timed_note(&mut stages, "clean1", || stage_clean(&netlist))?;
    timed_note(&mut stages, "fold", || stage_fold(&netlist))?;
    timed_note(&mut stages, "clean2", || stage_clean(&netlist))?;
    let verilog = timed(&mut stages, "emit", || stage_emit(&netlist))?;

    Ok(PipelineResult { stages, verilog })
}
