#!/usr/bin/env bash
# Usage: ./scripts/compare.sh [--csv]
#
# Compares safety-profiler vs yosys on all inputs/*.v
#
# All timings are in integer microseconds (µs) — consistent across both
# pipelines so ours-total and yosys-total can be compared directly.
#
# Flags:
#   --csv   Emit machine-readable CSV instead of the human table.
#           Useful for feeding into a spreadsheet or plotting script.
#
# Output columns:
#   circuit      Basename of the input file (no extension)
#   parse        Stage 1: sv_parser::parse_sv_str
#   compile      Stage 2: nl_compiler::from_vast
#   clean1       Stage 3: safety_pass::Clean (first pass)
#   fold         Stage 4: safety_pass::FoldAllPatterns
#   clean2       Stage 5: safety_pass::Clean (second pass)
#   emit         Stage 6: safety_pass::PrintVerilog
#   ours-total   Sum of the six stages above
#   yosys-total  Wall time of the full yosys invocation (includes startup)
#   speedup      yosys-total / ours-total  (>1 means we are faster)
#
# Note on Yosys timing: we measure the full yosys process wall time, which
# includes interpreter startup (~100-300ms on small inputs). This slightly
# favours us on tiny circuits but represents real CLI usage. See run_yosys.sh.

set -euo pipefail

BINARY="./target/release/safety-profiler"
INPUTS_DIR="inputs"
YOSYS_OUT_DIR="outputs/yosys"
OURS_OUT_DIR="outputs/ours"
CSV_MODE=0

[[ "${1:-}" == "--csv" ]] && CSV_MODE=1

mkdir -p "$YOSYS_OUT_DIR" "$OURS_OUT_DIR"

# ── Helpers ──────────────────────────────────────────────────────────────────

# extract <label> <output_blob>
# Pulls the integer µs value from a line like:  [label]  12345µs  optional msg
# Matches on the exact label (anchored with ^) to avoid clean1 matching clean2.
extract() {
    local label="$1"
    local output="$2"
    echo "$output" | grep "^\[${label}\]" | awk '{print $2}' | tr -d 'µs'
}

# ── Header ───────────────────────────────────────────────────────────────────

if [[ $CSV_MODE -eq 1 ]]; then
    echo "circuit,parse_us,compile_us,clean1_us,fold_us,clean2_us,emit_us,ours_total_us,yosys_total_us,speedup"
else
    echo "=== Pipeline Comparison (all times in µs) ==="
    echo ""
    printf "%-20s %10s %10s %10s %10s %10s %10s %13s %13s %8s\n" \
        "circuit" "parse" "compile" "clean1" "fold" "clean2" "emit" "ours-total" "yosys-total" "speedup"
    echo "$(printf '%0.s─' {1..115})"
fi

# ── Per-circuit loop ─────────────────────────────────────────────────────────

for INPUT in "$INPUTS_DIR"/*.v; do
    # Skip output files that get written next to their inputs by the binary.
    [[ "$INPUT" == *.out.v ]] && continue

    NAME=$(basename "$INPUT" .v)
    YOSYS_OUT="$YOSYS_OUT_DIR/${NAME}.v"
    OURS_OUT="$OURS_OUT_DIR/${NAME}.v"

    # ── Run our pipeline ───────────────────────────────────────────────────
    # stdout = timing lines; stderr = "Written to ..." message (discarded here)
    OURS_OUTPUT=$("$BINARY" "$INPUT" 2>/dev/null)

    # Move the output file to outputs/ours/ for diffing against Yosys later.
    cp "${INPUTS_DIR}/${NAME}.out.v" "$OURS_OUT" 2>/dev/null || true

    PARSE=$(extract   "parse"   "$OURS_OUTPUT")
    COMPILE=$(extract "compile" "$OURS_OUTPUT")
    CLEAN1=$(extract  "clean1"  "$OURS_OUTPUT")
    FOLD=$(extract    "fold"    "$OURS_OUTPUT")
    CLEAN2=$(extract  "clean2"  "$OURS_OUTPUT")
    EMIT=$(extract    "emit"    "$OURS_OUTPUT")

    # Sum all six stages. Values are plain integers after tr -d 'µs'.
    OURS_TOTAL=$(( ${PARSE:-0} + ${COMPILE:-0} + ${CLEAN1:-0} \
                 + ${FOLD:-0}  + ${CLEAN2:-0}  + ${EMIT:-0} ))

    # ── Run Yosys ─────────────────────────────────────────────────────────
    YOSYS_LINE=$(bash scripts/run_yosys.sh "$INPUT" "$YOSYS_OUT" 2>/dev/null)
    YOSYS_TIME=$(echo "$YOSYS_LINE" | awk '{print $2}' | tr -d 'µs')

    # ── Speedup ratio ─────────────────────────────────────────────────────
    if [[ -n "$YOSYS_TIME" && "${OURS_TOTAL}" -gt 0 ]]; then
        SPEEDUP=$(echo "scale=2; $YOSYS_TIME / $OURS_TOTAL" | bc)x
    else
        SPEEDUP="n/a"
    fi

    # ── Emit row ──────────────────────────────────────────────────────────
    if [[ $CSV_MODE -eq 1 ]]; then
        echo "${NAME},${PARSE},${COMPILE},${CLEAN1},${FOLD},${CLEAN2},${EMIT},${OURS_TOTAL},${YOSYS_TIME},${SPEEDUP}"
    else
        printf "%-20s %10s %10s %10s %10s %10s %10s %13s %13s %8s\n" \
            "$NAME" \
            "${PARSE}µs" "${COMPILE}µs" "${CLEAN1}µs" \
            "${FOLD}µs"  "${CLEAN2}µs"  "${EMIT}µs" \
            "${OURS_TOTAL}µs" "${YOSYS_TIME}µs" "$SPEEDUP"
    fi
done
