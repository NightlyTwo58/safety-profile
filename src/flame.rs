/// Flamegraph generation for the pipeline.
///
/// Why the old graphs were unhelpful: a circuit finishes in microseconds, so a
/// single run yields a handful of samples at best.  Here the pipeline is looped
/// for a fixed wall-clock budget so the graph has thousands of samples, and the
/// `stage_*` functions are `#[inline(never)]` so each stage shows up as its own
/// frame directly under `run_pipeline`.
///
/// Requires the `flamegraph` cargo feature (pprof is optional) and debug info;
/// use the `profiling` profile:
///
///   cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph
use std::{path::Path, time::Duration};

#[cfg(feature = "flamegraph")]
pub fn profile_pipeline(
    src: &str,
    path: &Path,
    budget: Duration,
    svg: &Path,
) -> Result<u64, String> {
    use crate::run_pipeline;
    use pprof::ProfilerGuardBuilder;
    use std::{fs::File, hint::black_box, time::Instant};

    // Warm caches and lazy statics outside the profile.
    run_pipeline(src, path).map_err(|e| e.to_string())?;

    let guard = ProfilerGuardBuilder::default()
        .frequency(997) // prime, avoids lockstep with periodic work
        .blocklist(&["libc", "libgcc", "pthread", "vdso"])
        .build()
        .map_err(|e| e.to_string())?;

    let start = Instant::now();
    let mut iters = 0u64;
    while start.elapsed() < budget {
        black_box(run_pipeline(black_box(src), path).map_err(|e| e.to_string())?);
        iters += 1;
    }

    let report = guard.report().build().map_err(|e| e.to_string())?;
    let name = path.file_stem().map(|s| s.to_string_lossy()).unwrap_or_default();

    let mut opts = pprof::flamegraph::Options::default();
    opts.title = format!("{name}: parse > compile > clean > fold > clean > emit");
    opts.subtitle = Some(format!("{iters} iterations over {:.1}s", budget.as_secs_f64()));

    let file = File::create(svg).map_err(|e| format!("{}: {e}", svg.display()))?;
    report
        .flamegraph_with_options(file, &mut opts)
        .map_err(|e| e.to_string())?;
    Ok(iters)
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
