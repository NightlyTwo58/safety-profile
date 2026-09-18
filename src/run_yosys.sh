#!/usr/bin/env bash
# Usage: ./scripts/run_yosys.sh <input.v> <output.v>
#
# Runs the Yosys flow equivalent to our safety-pass pipeline:
#   opt_expr  ≈  FoldAllPatterns
#   opt_clean ≈  Clean
#
# Output (single line to stdout):
#   [yosys]  <integer>µs  <input_path>
#
# The format must match what compare.sh's extract() function expects:
#   grep "^\[yosys\]" | awk '{print $2}' | tr -d 'µs'
#
# Timing notes:
#   - We measure the full wall time of the yosys process, including startup.
#   - On small circuits, yosys startup (~100–300 ms) dominates and inflates
#     the yosys-total figure relative to our pipeline.  This is intentional:
#     it reflects real-world CLI usage, and the effect diminishes on larger
#     circuits where pass time dominates.
#   - date +%s%N gives nanoseconds; dividing by 1000 gives microseconds.
#     On macOS, gdate (from coreutils) is required for %N support.

set -euo pipefail

INPUT="$1"
OUTPUT="$2"

# macOS compatibility: use gdate if available, fall back to date.
DATE_CMD="date"
if command -v gdate &>/dev/null; then
    DATE_CMD="gdate"
fi

START=$($DATE_CMD +%s%N)

yosys -q -p "
    read_verilog $INPUT;
    opt_expr;
    opt_clean;
    write_verilog -noattr $OUTPUT
"

END=$($DATE_CMD +%s%N)
ELAPSED_US=$(( (END - START) / 1000 ))

echo "[yosys]  ${ELAPSED_US}µs  $INPUT"
