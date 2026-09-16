#!/usr/bin/env bash
# Usage: ./scripts/run_yosys.sh <input.v> <output.v>
# Runs the equivalent optimization flow to our safety-pass pipeline

set -euo pipefail

INPUT="$1"
OUTPUT="$2"

START=$(date +%s%N)

yosys -q -p "
    read_verilog $INPUT;
    opt_expr;
    opt_clean;
    write_verilog -noattr $OUTPUT
"

END=$(date +%s%N)
ELAPSED=$(( (END - START) / 1000 ))
echo "[yosys]    ${ELAPSED}µs   $INPUT"