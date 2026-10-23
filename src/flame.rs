/// Flamegraph generation for the pipeline.
///
/// Why the old graphs were unhelpful: a circuit finishes in microseconds, so a
/// single run yields a handful of samples at best.  Here the pipeline is looped
/// for a fixed wall-clock budget so the graph has enough samples. Named group
/// frames separate frontend, optimization, and output work; each `stage_*`
/// function remains a child frame for pass-level detail.
///
/// Requires the `flamegraph` cargo feature (pprof is optional) and debug info;
/// use the `profiling` profile:
///
///   cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph
use std::{path::Path, time::Duration};

#[cfg(feature = "flamegraph")]
use crate::{PipelineError, stage_clean, stage_compile, stage_emit, stage_fold, stage_parse};
#[cfg(feature = "flamegraph")]
use safety_net::Netlist;
#[cfg(feature = "flamegraph")]
use safety_pass::Cell;
#[cfg(feature = "flamegraph")]
use std::rc::Rc;

#[cfg(feature = "flamegraph")]
pub fn profile_pipeline(
    src: &str,
    path: &Path,
    budget: Duration,
    svg: &Path,
) -> Result<u64, String> {
    use pprof::ProfilerGuardBuilder;
    use std::{fs::File, hint::black_box, time::Instant};

    // Warm caches and lazy statics outside the profile.
    profile_iteration(src, path).map_err(|e| e.to_string())?;

    let guard = ProfilerGuardBuilder::default()
        .frequency(997) // prime, avoids lockstep with periodic work
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|e| e.to_string())?;

    let start = Instant::now();
    let mut iters = 0u64;
    while start.elapsed() < budget {
        black_box(profile_iteration(black_box(src), path).map_err(|e| e.to_string())?);
        iters += 1;
    }

    let report = guard.report().build().map_err(|e| e.to_string())?;
    let name = path
        .file_stem()
        .map(|s| s.to_string_lossy())
        .unwrap_or_default();

    let mut opts = pprof::flamegraph::Options::default();
    opts.title = format!("Pipeline CPU profile: {name}");
    opts.subtitle = Some(format!(
        "{iters} runs | Frontend: parse + compile | Optimize: clean + fold + clean | Output: emit | {:.1}s",
        budget.as_secs_f64()
    ));

    let file = File::create(svg).map_err(|e| format!("{}: {e}", svg.display()))?;
    report
        .flamegraph_with_options(file, &mut opts)
        .map_err(|e| e.to_string())?;
    Ok(iters)
}

#[cfg(feature = "flamegraph")]
type Net = Rc<Netlist<Cell>>;

/// One profiled pipeline run with stable, named group frames in the call tree.
#[cfg(feature = "flamegraph")]
fn profile_iteration(src: &str, path: &Path) -> Result<String, PipelineError> {
    let netlist = profile_group_frontend(src, path)?;
    profile_group_optimization(&netlist)?;
    profile_group_output(&netlist)
}

#[cfg(feature = "flamegraph")]
#[inline(never)]
fn profile_group_frontend(src: &str, path: &Path) -> Result<Net, PipelineError> {
    let ast = stage_parse(src, path)?;
    stage_compile(&ast)
}

#[cfg(feature = "flamegraph")]
#[inline(never)]
fn profile_group_optimization(netlist: &Net) -> Result<(), PipelineError> {
    let _ = stage_clean(netlist)?;
    let _ = stage_fold(netlist)?;
    let _ = stage_clean(netlist)?;
    Ok(())
}

#[cfg(feature = "flamegraph")]
#[inline(never)]
fn profile_group_output(netlist: &Net) -> Result<String, PipelineError> {
    stage_emit(netlist)
}

#[cfg(not(feature = "flamegraph"))]
pub fn profile_pipeline(
    _src: &str,
    _path: &Path,
    _budget: Duration,
    _svg: &Path,
) -> Result<u64, String> {
    Err("built without the `flamegraph` feature; rebuild with --features flamegraph".into())
}
