
# Safety-Profile
 
Benchmarks the [Safety-Net](https://github.com/matth2k/safety-net) /
[Safety-Pass](https://github.com/matth2k/safety-pass) /
[NL-Compiler](https://github.com/matth2k/nl-compiler) optimization pipeline
against a [Yosys](https://github.com/YosysHQ/yosys) baseline. [EqMap](https://github.com/cornell-zhang/eqmap) is used only to
preprocess inputs.
 
## Measurements
 
Each circuit runs through six timed stages. File reads are never timed.
 
| Stage     | Work                                            |
|-----------|-------------------------------------------------|
| `parse`   | Verilog text to AST (`sv-parser`)               |
| `compile` | AST to netlist, first module only               |
| `clean1`  | Dead cell and wire removal                      |
| `fold`    | Constant and pattern folding                    |
| `clean2`  | Second clean                                    |
| `emit`    | Netlist to Verilog                              |
 
`RenameNets` is timed separately, on a freshly compiled netlist.
 
Yosys baseline: `read_verilog; opt_expr; opt_clean; write_verilog`.
Yosys runs as a subprocess, so its time includes startup. Startup is measured
once and two speedups are reported: `x-raw` (Yosys total / ours) and `x-net`
(Yosys total minus startup / ours). Prefer `x-net` on small circuits.
 
Rename baseline: `rename -hide *; rename -enumerate -pattern __%__ *`, timed
from Yosys's own `-d` report (no parse or startup). Small inputs repeat the
pair per sample for timer resolution; the count is in the CSV.
 
All reported numbers are medians; CSVs also include min and standard deviation.

## Quick start
 
```
# Put .v files in inputs/raw/ and techmap.v in inputs/
cargo run --release --bin preprocess
cargo run --release --bin compare
```
 
 ```sh
cargo run --release --bin preprocess -- [RAW_DIR] [OUT_DIR] [--techmap FILE] [-k N]
```
  
```sh
cargo run --release --bin compare -- [PATH...] [options]
```

For example,
```sh
cargo run --release --bin compare -- inputs/c17.v inputs/c432.v
cargo run --release --bin compare -- -f c4 -i 100
cargo run --release --bin compare -- --csv > results.csv
```


| Option                | Purpose                                         | Default  |
|-----------------------|-------------------------------------------------|----------|
| `-f, --filter <text>` | Only circuits whose name contains `<text>`      | none     |
| `--exclude <text>`    | Skip circuits whose name contains `<text>` (repeatable) | none |
| `-i, --iters <n>`     | Timed iterations per circuit and tool           | 20       |
| `--warmup <n>`        | Untimed warmup iterations                       | 3        |
| `-o, --out-dir <dir>` | Root for generated files                        | `output` |
| `--csv`               | Print the pipeline CSV to stdout, not the table | off      |
| `--no-yosys`          | Our pipeline only; drops all Yosys columns      | off      |
| `--flamegraph`        | Write a flamegraph per circuit                  | off      |
| `--flame-secs <s>`    | Sampling time per flamegraph                    | 2.0      |
 
## More Features
```sh
cargo run --release --bin safety-profiler -- inputs/c17.v [-i N] [--flamegraph] [--save-verilog]
```
 
```sh
cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph --exclude gnd
```
 
```sh
cargo bench
cargo bench --features flamegraph
```
 
```sh
cargo build --profile profiling
samply record ./target/profiling/safety-profiler inputs/c17.v -i 2000
```
 
## Code layout
 
| Path                    | Role                                              |
|-------------------------|---------------------------------------------------|
| `src/lib.rs`            | Pipeline stages, `run_pipeline`, `run_rename_nets`|
| `src/io.rs`             | Input discovery, output directory layout          |
| `src/stats.rs`          | Median, min, mean, standard deviation             |
| `src/yosys.rs`          | Yosys subprocess timing and normalization         |
| `src/report.rs`         | Comparison tables and CSV output                  |
| `src/flame.rs`          | Flamegraphs     |
| `src/main.rs`           | `safety-profiler` CLI                             |
| `src/bin/compare.rs`    | Comparison harness                                |
| `src/bin/preprocess.rs` | ISCAS-85 normalization                            |
| `benches/pipeline.rs`   | Criterion per-stage benches                       |
 
