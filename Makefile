# Entry points for the three workflows. Extra flags: make regress ARGS="-f c4 -i 50"
ARGS ?=

.PHONY: netlists regress analyze plot mem rename test bless flame bench

netlists:
	cargo run --release --bin preprocess -- $(ARGS)

regress:
	cargo run --release --bin compare -- --exclude gnd $(ARGS)

analyze:
	python3 scripts/analyze.py

plot:
	python3 scripts/analyze.py --plot

mem:
	cargo run --release --bin memprof -- --exclude gnd $(ARGS)

rename:
	cargo run --release --bin rename -- --exclude gnd $(ARGS)

test:
	cargo test --release --test regression

bless:
	BLESS=1 cargo test --release --test regression

flame:
	cargo run --profile profiling --features flamegraph --bin compare -- --flamegraph --exclude gnd $(ARGS)

bench:
	cargo bench
