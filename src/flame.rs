//! Pipeline flamegraphs.
//!
//! A circuit finishes in microseconds, so one run yields almost no samples.
//! The pipeline is looped for a wall-clock budget instead. Named group frames
//! (Frontend, Optimize, Output) sit above the per-stage frames.
//!
//! Needs the `flamegraph` feature and debug info (the `profiling` profile):
//!
//! ```sh
//! cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph
//! ```

#[cfg(feature = "flamegraph")]
pub use enabled::profile_pipeline;

#[cfg(feature = "flamegraph")]
mod enabled {
    use crate::{
        io::stem, stage_clean, stage_compile, stage_emit, stage_fold, stage_parse, Net,
        PipelineError,
    };
    use pprof::ProfilerGuardBuilder;
    use std::{
        fs::File,
        hint::black_box,
        path::Path,
        time::{Duration, Instant},
    };

    /// Loop the pipeline for `budget`, then write a flamegraph to `svg`.
    ///
    /// Returns the number of iterations run.
    pub fn profile_pipeline(
        src: &str,
        path: &Path,
        budget: Duration,
        svg: &Path,
    ) -> Result<u64, String> {
        profile_iteration(src, path).map_err(|e| e.to_string())?;

        let guard = ProfilerGuardBuilder::default()
            .frequency(997)
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
        let mut opts = pprof::flamegraph::Options::default();
        opts.title = format!("Pipeline CPU profile: {}", stem(path));
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

    /// One pipeline run, split into named group frames.
    fn profile_iteration(src: &str, path: &Path) -> Result<String, PipelineError> {
        let netlist = frontend(src, path)?;
        optimize(&netlist)?;
        output(&netlist)
    }

    /// Group frame: parse and compile.
    #[inline(never)]
    fn frontend(src: &str, path: &Path) -> Result<Net, PipelineError> {
        let ast = stage_parse(src, path)?;
        stage_compile(&ast)
    }

    /// Group frame: clean, fold, clean.
    #[inline(never)]
    fn optimize(netlist: &Net) -> Result<(), PipelineError> {
        stage_clean(netlist)?;
        stage_fold(netlist)?;
        stage_clean(netlist)?;
        Ok(())
    }

    /// Group frame: emit.
    #[inline(never)]
    fn output(netlist: &Net) -> Result<String, PipelineError> {
        stage_emit(netlist)
    }
}

/// Stub used when the `flamegraph` feature is off; always returns an error.
#[cfg(not(feature = "flamegraph"))]
pub fn profile_pipeline(
    _src: &str,
    _path: &std::path::Path,
    _budget: std::time::Duration,
    _svg: &std::path::Path,
) -> Result<u64, String> {
    Err("built without the `flamegraph` feature; rebuild with --features flamegraph".into())
}
