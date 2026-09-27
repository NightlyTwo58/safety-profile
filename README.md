# Safety-Profile
Benchmarking the performance of 
[Safety-Net](https://github.com/matth2k/safety-net), 
[Safety-Pass](https://github.com/matth2k/safety-pass), 
[EqMap](https://github.com/cornell-zhang/eqmap), and
[NL-Compiler](https://github.com/matth2k/nl-compiler) compared to Yosys basis.

### Convert canonical ISCAS-85 that compatible Safety-Net and Yosys starting point
`cargo run --bin preprocess --release`
### Run the comparison table
`cargo run --bin compare --release`
### CSV for plotting
`cargo run --bin compare --release -- --csv > results.csv`
### Per-stage Criterion benches
`cargo bench`
### Flamegraphs
`cargo bench --features flamegraph`
### Quick profiling run with samply
`cargo build --profile profiling`
`samply record ./target/profiling/safety-profiler inputs/c17.v`