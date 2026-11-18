# Safety-Profile

Benchmarks the [Safety-Net](https://github.com/matth2k/safety-net) /
[Safety-Pass](https://github.com/matth2k/safety-pass) /
[NL-Compiler](https://github.com/matth2k/nl-compiler) pass pipeline against a
[Yosys](https://github.com/YosysHQ/yosys) baseline.
[EqMap](https://github.com/cornell-zhang/eqmap) is used only to generate inputs.

Requires a Rust toolchain, `yosys` on `PATH`, and Python 3 for analysis.

## Workflow

| Task | Command | Result |
|------|---------|--------|
| Generate netlists | `make netlists` | `inputs/*.v` from `inputs/raw/*.v` + `inputs/techmap.v` |
| Benchmark vs Yosys | `make regress` | summary table, CSVs in `output/stats/` |
| Profile memory | `make mem` | peak heap per stage, `output/stats/memory.csv` |
| Rename netlists | `make rename` | `output/verilog/renamed/*.v` |
| Test for regressions | `make test` (`make bless` to update) | snapshot comparison |
| Analyze | `make analyze` (`make plot` for PNGs) | speedups, stage shares, peak heap |

Pass extra flags with `ARGS`, e.g. `make regress ARGS="-f c4 -i 50"`.

### Generate netlists (`preprocess`)

Yosys maps raw verilog primitives to named cells via the techmap, then EqMap
LUT-packs them.

```sh
cargo run --release --bin preprocess -- [RAW_DIR] [OUT_DIR] [--techmap FILE] [-k N]
```

Defaults: `RAW_DIR=inputs/raw`, `OUT_DIR=inputs`, techmap `OUT_DIR/techmap.v`, `k=6`.

### Benchmark (`compare`)

Runs every circuit through the pipeline and Yosys. Exits nonzero if any circuit
fails. The terminal shows one line per circuit; everything else goes to CSV.

```sh
cargo run --release --bin compare -- [PATH...] [options]
```

`PATH` is any mix of `.v` files and directories (default `inputs`).

| Option | Purpose | Default |
|--------|---------|---------|
| `-f, --filter <text>` | Only circuits whose name contains `<text>` | none |
| `--exclude <text>` | Skip matching circuits (repeatable) | none |
| `-i, --iters <n>` | Timed iterations per circuit and tool | 20 |
| `--warmup <n>` | Untimed warmup iterations | 3 |
| `-o, --out-dir <dir>` | Root for generated files | `output` |
| `--stages` | Add per-stage columns to the table | off |
| `--rename` | Print the rename comparison table | off |
| `--csv` | Print the pipeline CSV instead of the table | off |
| `--no-yosys` | Our pipeline only | off |
| `--flamegraph` | Write a flamegraph per circuit | off |
| `--flame-secs <s>` | Sampling time per flamegraph | 2.0 |

`gnd.v` holds an `X` constant that `Cell` does not support; `make regress` excludes it.

Other runners:

```sh
cargo run --release --bin safety-profiler -- inputs/c17.v [-i N] [--flamegraph] [--save-verilog]
cargo bench                           # Criterion per-stage benches
make flame                            # flamegraphs (needs `profiling` profile)
```

### Profile memory

`memprof` counts heap allocations with a tracking allocator and reports, per stage, peak
heap above the stage's starting level and the heap still live afterward.
It runs separately from `compare` so the allocator does not affect timings.

### Rename netlists ()

`rename` applies `RenameNets` (sequential net and instance names) and writes the emitted
Verilog. Same `PATH`, `-f`, `--exclude`, `-o` options as `compare`.

### Regression Tests

`tests/regression.rs` compares pipeline and rename output for every input with
snapshots in `tests/golden/`. After an intended change, run `make bless` and
commit the diff.

### Analysis

`scripts/analyze.py` reads `output/stats/*.csv` and prints geometric-mean, min, and max speedups,
the slowest circuits, peak heap, and each stage's share of pipeline time. `--plot` writes
`speedup.png` and `stage_share.png` (needs matplotlib).

## Methodology

- Six timed stages: `parse`, `compile`, `clean1`, `fold`, `clean2`, `emit`. File reads are untimed.
- `RenameNets` is timed separately on a freshly compiled netlist.
- Yosys baseline: `read_verilog; opt_expr; opt_clean; write_verilog`. It runs as a subprocess,
  so startup is measured once: `x-raw` = Yosys / ours, `x-net` = (Yosys - startup) / ours.
  Prefer `x-net` on small circuits.
- Rename baseline: `rename -hide *; rename -enumerate -pattern __%__ *`, timed from Yosys's `-d`
  report. Small inputs repeat the pair per sample; the count is in the CSV.
- Reported values are medians; CSVs also hold min and standard deviation.

## Output

```text
output/
  stats/        compare.csv, compare_stages.csv, compare_rename.csv
  flamegraphs/  <circuit>.pipeline.svg
  verilog/      ours/ and yosys/ netlists
```

## Layout

| Path | Role |
|------|------|
| `src/lib.rs` | Pipeline stages, `run_pipeline`, `run_rename_nets` |
| `src/io.rs` | Input discovery, output layout |
| `src/yosys.rs` | Yosys timing and normalization |
| `src/report.rs` | Tables and CSV output |
| `src/stats.rs`, `src/mem.rs`, `src/flame.rs` | Statistics, heap tracking, flamegraphs |
| `src/main.rs`, `src/bin/*.rs` | `safety-profiler`, `compare`, `preprocess`, `memprof`, `rename` |
| `tests/regression.rs` | Snapshot tests |
| `benches/pipeline.rs` | Criterion benches |
| `scripts/analyze.py`, `Makefile` | Analysis, workflow entry points |
