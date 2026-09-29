# Safety-Profile

Benchmarks the optimization pipeline built from
[Safety-Net](https://github.com/matth2k/safety-net),
[Safety-Pass](https://github.com/matth2k/safety-pass),
[EqMap](https://github.com/cornell-zhang/eqmap), and
[NL-Compiler](https://github.com/matth2k/nl-compiler) against a
[Yosys](https://github.com/YosysHQ/yosys) baseline on the ISCAS-85 circuits.

## What is measured

Each circuit goes through six timed stages:

```
parse -> compile -> clean1 -> fold -> clean2 -> emit
```

| Stage     | What it does                                          |
|-----------|-------------------------------------------------------|
| `parse`   | Verilog text to AST (`sv-parser`)                     |
| `compile` | AST to netlist, first module only (`nl-compiler`)     |
| `clean1`  | Dead-cell and dead-wire removal                       |
| `fold`    | Constant and pattern folding                          |
| `clean2`  | Second clean after folding                            |
| `emit`    | Netlist back to Verilog                               |

Yosys runs the equivalent flow: `read_verilog; opt_expr; opt_clean; write_verilog`.
File reads are never timed.

**Yosys startup:** Yosys runs as a subprocess, so its wall time includes process
startup. `compare` measures that startup once and reports two speedups:
`x-raw` (Yosys total over ours) and `x-net` (Yosys total minus startup, over ours).
`x-net` is the fairer number on small circuits.

## Requirements

- Rust (edition 2024 toolchain)
- `yosys` on your `PATH`
- Optional: [`samply`](https://github.com/mstange/samply) for sampling profiles

## Quick start

```sh
# 1. Put the unmodified ISCAS-85 .v files in inputs/raw/ and copy techmap.v to inputs/
cargo run --release --bin preprocess

# 2. Compare against Yosys
cargo run --release --bin compare
```

Results are printed as a table and saved under `output/` (see below).

## Commands

### `preprocess`: normalize the raw ISCAS-85 files

Runs Yosys with a techmap, then EqMap LUT packing, so the circuits are valid
input for both Safety-Net and Yosys.

```sh
cargo run --release --bin preprocess -- [RAW_DIR] [OUT_DIR] [--techmap FILE] [-k N]
```

Defaults: `RAW_DIR=inputs/raw`, `OUT_DIR=inputs`, techmap `OUT_DIR/techmap.v`, `k=6`.

### `compare`: our pipeline vs Yosys

```sh
cargo run --release --bin compare -- [PATH...] [options]
```

`PATH` is any mix of `.v` files and directories (default: `inputs`).
Generated files (`*.out.v`, `*.normalized.v`) and `techmap.v` are skipped when
a directory is expanded.

| Option                  | Meaning                                              | Default  |
|-------------------------|------------------------------------------------------|----------|
| `-f, --filter <text>`   | Only circuits whose name contains `<text>`           | none     |
| `-i, --iters <n>`       | Timed iterations per circuit and tool                | 20       |
| `--warmup <n>`          | Untimed warmup iterations                            | 3        |
| `-o, --out-dir <dir>`   | Root directory for all generated files               | `output` |
| `--csv`                 | Print CSV to stdout instead of the table             | off      |
| `--no-yosys`            | Run our pipeline only                                | off      |
| `--flamegraph`          | Also write a flamegraph per circuit (needs feature)  | off      |
| `--flame-secs <s>`      | Sampling time per flamegraph                         | 2.0      |

Examples:

```sh
cargo run --release --bin compare -- inputs/c17.v inputs/c432.v   # two circuits
cargo run --release --bin compare -- -f c4 -i 100                  # names containing "c4"
cargo run --release --bin compare -- --csv > results.csv           # CSV on stdout
```

Reported numbers are medians over the timed iterations. The CSVs also include
min and standard deviation.

### `safety-profiler`: run one circuit

```sh
cargo run --release --bin safety-profiler -- inputs/c17.v [-i N] [--flamegraph]
```

Prints per-stage timings for one file. With `-i N` it runs N times and reports
median, min and standard deviation.

## Output layout

Everything generated goes under `output/` (never `target/`):

```
output/
  stats/
    compare.csv          one row per circuit: stage medians, totals, speedups
    compare_stages.csv   one row per circuit and stage, with % of total time
  flamegraphs/
    <circuit>.svg
  verilog/
    ours/                last netlist emitted by our pipeline
    yosys/               last netlist emitted by Yosys, for diffing
```

Change the root with `--out-dir`.

## Flamegraphs

Flamegraphs need the optional `flamegraph` feature (it pulls in `pprof`) and
debug symbols, so use the `profiling` profile:

```sh
cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph
cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph -f c432
```

Circuits finish in microseconds, so each flamegraph loops the pipeline for
`--flame-secs` to collect enough samples. Each stage appears as its own frame
directly under `run_pipeline`, so the width of a frame is that stage's share of
the time. Without the feature, `--flamegraph` prints an error and continues.

## Criterion benches

Per-stage benchmarks, with setup excluded from the timing:

```sh
cargo bench
cargo bench --features flamegraph     # also produces flamegraphs
```

Criterion reports go to `target/criterion/`.

## Sampling with samply

```sh
cargo build --profile profiling
samply record ./target/profiling/safety-profiler inputs/c17.v -i 2000
```

Use `-i` with a large count so the run lasts long enough to sample.

## Code layout

| Path                  | Role                                                       |
|-----------------------|------------------------------------------------------------|
| `src/lib.rs`          | Pipeline stages and `run_pipeline` (single source of truth)|
| `src/io.rs`           | Input discovery and output directory layout                |
| `src/stats.rs`        | Median, min, standard deviation                            |
| `src/flame.rs`        | Flamegraph generation (behind the `flamegraph` feature)    |
| `src/main.rs`         | `safety-profiler` CLI                                      |
| `src/bin/compare.rs`  | Comparison harness                                         |
| `src/bin/preprocess.rs` | ISCAS-85 normalization                                   |
| `benches/pipeline.rs` | Criterion per-stage benches                                |
