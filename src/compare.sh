#!/usr/bin/env bash
# Usage: ./scripts/compare.sh
# Compares safety-profiler vs yosys on all inputs/*.v

set -euo pipefail

BINARY="./target/release/safety-profiler"
INPUTS_DIR="inputs"
YOSYS_OUT_DIR="outputs/yosys"
OURS_OUT_DIR="outputs/ours"

mkdir -p "$YOSYS_OUT_DIR" "$OURS_OUT_DIR"

echo "=== Pipeline Comparison ==="
echo ""
printf "%-20s %12s %12s %12s %12s %12s %12s\n" \
    "circuit" "ours-parse" "ours-compile" "ours-clean" "ours-fold" "ours-emit" "yosys-total"
echo "----------------------------------------------------------------------------------------------------"

for INPUT in "$INPUTS_DIR"/*.v; do
    # skip already-generated output files
    [[ "$INPUT" == *.out.v ]] && continue

    NAME=$(basename "$INPUT" .v)
    YOSYS_OUT="$YOSYS_OUT_DIR/${NAME}.v"
    OURS_OUT="$OURS_OUT_DIR/${NAME}.v"

    # Run ours, capture per-stage output
    OURS_OUTPUT=$("$BINARY" "$INPUT" 2>&1)
    # redirect output file to outputs/ours/
    cp "$(dirname "$INPUT")/${NAME}.out.v" "$OURS_OUT" 2>/dev/null || true

    PARSE=$(echo  "$OURS_OUTPUT" | grep '\[parse\]'   | awk '{print $2}')
    COMPILE=$(echo "$OURS_OUTPUT" | grep '\[compile\]' | awk '{print $2}')
    CLEAN=$(echo  "$OURS_OUTPUT" | grep '\[clean\]'   | head -1 | awk '{print $2}')
    FOLD=$(echo   "$OURS_OUTPUT" | grep '\[fold\]'    | awk '{print $2}')
    EMIT=$(echo   "$OURS_OUTPUT" | grep '\[emit\]'    | awk '{print $2}')

    # Run yosys, capture total time
    YOSYS_LINE=$(bash scripts/run_yosys.sh "$INPUT" "$YOSYS_OUT")
    YOSYS_TIME=$(echo "$YOSYS_LINE" | awk '{print $2}')

    printf "%-20s %12s %12s %12s %12s %12s %12s\n" \
        "$NAME" "$PARSE" "$COMPILE" "$CLEAN" "$FOLD" "$EMIT" "$YOSYS_TIME"
done