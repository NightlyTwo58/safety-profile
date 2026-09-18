[Safety-Net](https://github.com/matth2k/safety-net)
[Safety-Pass](https://github.com/matth2k/safety-pass)
[EqMap](https://github.com/cornell-zhang/eqmap)
[NL-Compiler](https://github.com/matth2k/nl-compiler)

# 1 Convert canonical ISCAS-85 in inputs/ that both tools accept
`./scripts/preprocess.sh inputs/raw inputs/`

# 2 Build profiler binary
`cargo build --release`

# 3 Run comparison table
`./scripts/compare.sh`
`./scripts/compare.sh --csv > results.csv`

# 4 Flamegraphs
`cargo install samply`
`samply record ./target/release/safety-profiler inputs/c17.v`

# 5 Per-stage Criterion benches
`cargo bench`
`cargo bench --features flamegraph`