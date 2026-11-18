//! Pipeline library for safety-profiler.
//!
//! Single source of truth for pipeline execution, shared by the
//! `safety-profiler` CLI, the `compare` harness, and the Criterion benches.
//!
//! ```text
//! Verilog source
//!   -> parse    sv_parser::parse_sv_str
//!   -> compile  nl_compiler::from_vast (first module)
//!   -> clean1   safety_pass::Clean
//!   -> fold     safety_pass::FoldAllPatterns
//!   -> clean2   safety_pass::Clean
//!   -> emit     safety_pass::PrintVerilog
//! Verilog output
//! ```
//!
//! `RenameNets` is measured separately by [`run_rename_nets`].

pub mod flame;
pub mod io;
pub mod report;
pub mod stats;
pub mod yosys;

use nl_compiler::from_vast;
use safety_pass::passes::{Clean, FoldAllPatterns, PrintVerilog, RenameNets};
use safety_pass::{Cell, Pass};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};
use sv_parser::{parse_sv_str, SyntaxTree};

/// Shared handle to a compiled netlist.
pub type Net = Rc<safety_net::Netlist<Cell>>;

/// Boxed error returned by every pipeline stage.
pub type PipelineError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Stage names in execution order, matching [`run_pipeline`].
pub const STAGE_NAMES: [&str; 6] = ["parse", "compile", "clean1", "fold", "clean2", "emit"];

/// Timing and optional note for one stage.
#[derive(Debug, Clone)]
pub struct StageTiming {
    pub name: &'static str,
    pub elapsed: Duration,
    /// Message returned by the pass, if it produces one.
    pub note: Option<String>,
}

impl StageTiming {
    pub fn micros(&self) -> u128 {
        self.elapsed.as_micros()
    }
}

/// Result of one full pipeline run.
#[derive(Debug)]
pub struct PipelineResult {
    /// One entry per stage, in execution order.
    pub stages: Vec<StageTiming>,
    /// Verilog emitted by the final stage.
    pub verilog: String,
}

impl PipelineResult {
    /// Sum of all stage timings in microseconds.
    pub fn total_micros(&self) -> u128 {
        self.stages.iter().map(StageTiming::micros).sum()
    }

    /// Look up a stage by name.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not in [`STAGE_NAMES`].
    pub fn stage(&self, name: &str) -> &StageTiming {
        self.stages
            .iter()
            .find(|s| s.name == name)
            .unwrap_or_else(|| panic!("no stage named {name:?}"))
    }
}

/// Parse Verilog source into a syntax tree.
///
/// `path` is used for diagnostics only. Stages are `#[inline(never)]` so each
/// appears as its own flamegraph frame.
#[inline(never)]
pub fn stage_parse(src: &str, path: &Path) -> Result<SyntaxTree, PipelineError> {
    let (ast, _) = parse_sv_str(
        src,
        path.to_path_buf(),
        &HashMap::new(),
        &[] as &[PathBuf],
        true,
        false,
    )?;
    Ok(ast)
}

/// Compile a syntax tree into a netlist of its first module.
#[inline(never)]
pub fn stage_compile(ast: &SyntaxTree) -> Result<Net, PipelineError> {
    let netlist = from_vast::<Cell>(ast)?
        .into_iter()
        .next()
        .ok_or("no modules found in file")?;
    Ok(netlist)
}

/// Remove dead cells and wires.
#[inline(never)]
pub fn stage_clean(netlist: &Net) -> Result<String, PipelineError> {
    Ok(Clean(PhantomData::<Cell>).run(netlist)?)
}

/// Fold constant and pattern-matchable expressions.
#[inline(never)]
pub fn stage_fold(netlist: &Net) -> Result<String, PipelineError> {
    Ok(FoldAllPatterns.run(netlist)?)
}

/// Emit Verilog from the netlist.
#[inline(never)]
pub fn stage_emit(netlist: &Net) -> Result<String, PipelineError> {
    Ok(PrintVerilog(PhantomData::<Cell>).run(netlist)?)
}

/// Rename nets and instances sequentially.
#[inline(never)]
pub fn stage_rename_nets(netlist: &Net) -> Result<String, PipelineError> {
    Ok(RenameNets(PhantomData::<Cell>).run(netlist)?)
}

/// Time `f` and record it under `name`.
fn timed<T>(
    stages: &mut Vec<StageTiming>,
    name: &'static str,
    f: impl FnOnce() -> Result<T, PipelineError>,
) -> Result<T, PipelineError> {
    let start = Instant::now();
    let value = f()?;
    stages.push(StageTiming {
        name,
        elapsed: start.elapsed(),
        note: None,
    });
    Ok(value)
}

/// Time a pass whose returned `String` is a note.
fn timed_note(
    name: &'static str,
    f: impl FnOnce() -> Result<String, PipelineError>,
) -> Result<StageTiming, PipelineError> {
    let start = Instant::now();
    let note = f()?;
    Ok(StageTiming {
        name,
        elapsed: start.elapsed(),
        note: Some(note),
    })
}

/// Run the full pipeline on already-loaded source.
///
/// File I/O is the caller's job so it never enters the timings.
pub fn run_pipeline(src: &str, path: &Path) -> Result<PipelineResult, PipelineError> {
    let mut stages = Vec::with_capacity(STAGE_NAMES.len());

    let ast = timed(&mut stages, "parse", || stage_parse(src, path))?;
    let netlist = timed(&mut stages, "compile", || stage_compile(&ast))?;
    stages.push(timed_note("clean1", || stage_clean(&netlist))?);
    stages.push(timed_note("fold", || stage_fold(&netlist))?);
    stages.push(timed_note("clean2", || stage_clean(&netlist))?);
    let verilog = timed(&mut stages, "emit", || stage_emit(&netlist))?;

    Ok(PipelineResult { stages, verilog })
}

/// Time `RenameNets` on a freshly compiled netlist.
///
/// Parsing and compilation are untimed setup.
pub fn run_rename_nets(src: &str, path: &Path) -> Result<StageTiming, PipelineError> {
    let ast = stage_parse(src, path)?;
    let netlist = stage_compile(&ast)?;
    timed_note("rename_nets", || stage_rename_nets(&netlist))
}
