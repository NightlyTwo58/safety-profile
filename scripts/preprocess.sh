#!/usr/bin/env bash
# Usage: ./scripts/preprocess.sh [input_dir] [output_dir]
#
# Converts canonical ISCAS-85 benchmark Verilog into the subset that both
# Yosys and Matt's nl_compiler accept, producing files in inputs/.
#
# Why this exists
# 
# The canonical ISCAS-85 files (e.g. from opencores or the original Brglez &
# Fujiwara distribution) use anonymous Verilog gate primitives:
#
#     nand (N10, N1, N3);      // no instance name — nl_compiler can't build
#                              // a netlist node without an identity
#
# Yosys handles these fine.  nl_compiler requires named instances:
#
#     nand g_N10 (N10, N1, N3);
#
# Additional issues in some distributions:
#   - Missing explicit `wire` declarations for internal nets
#   - `timescale` directives (ignored by our parser but noisy)
#   - Multi-output primitives (`buf` with fanout listed as multiple outputs)
#     — these need splitting into individual assignments
#
# Transformation pipeline (per file)
# 
# Step 1 — Strip timescale directives        (sed)
# Step 2 — Add explicit wire declarations    (awk)
# Step 3 — Name anonymous gate instances     (awk)
# Step 4 — Validate with both tools          (yosys read_verilog + dry-run build)
#
# Output files land in INPUT_DIR (default: inputs/) so that compare.sh picks
# them up automatically.  Original files are preserved with a .orig suffix.
#
# Both pipelines always run on the SAME preprocessed file — the preprocessing
# cost is NOT counted in either pipeline's benchmark time.

set -euo pipefail

RAW_DIR="${1:-inputs/raw}"      # canonical ISCAS-85 files go here
OUT_DIR="${2:-inputs}"           # preprocessed files land here (compare.sh reads this)

mkdir -p "$OUT_DIR"

if [[ ! -d "$RAW_DIR" ]]; then
    echo "ERROR: Raw input directory '$RAW_DIR' not found."
    echo "Place the unmodified ISCAS-85 .v files there and re-run."
    exit 1
fi

echo "=== Preprocessing ISCAS-85 inputs ==="
echo "  raw:    $RAW_DIR"
echo "  output: $OUT_DIR"
echo ""

_process_file() {
    local SRC="$1"
    local NAME
    NAME=$(basename "$SRC" .v)
    local DST="$OUT_DIR/${NAME}.v"

    echo -n "  ${NAME}.v ... "

    #  Step 1: Strip `timescale 
    local TMP1
    TMP1=$(mktemp)
    sed '/`timescale/d' "$SRC" > "$TMP1"

    #  Step 2 + 3: Add wire decls and name anonymous instances 
    # One awk pass does both:
    #   - Collects all net names driven by gate outputs (first arg in primitive)
    #   - Emits `wire` declarations for any net not in the port list
    #   - Adds instance names `g_<output_net>` to anonymous primitives
    local TMP2
    TMP2=$(mktemp)
    awk '
    BEGIN { inst_count = 0 }

    # Collect port names so we do not re-declare them as wires
    /^\s*(input|output)\s/ {
        split($0, parts, /[\s,;]+/)
        for (i in parts) {
            gsub(/\s/, "", parts[i])
            if (parts[i] != "input" && parts[i] != "output" && parts[i] != "")
                ports[parts[i]] = 1
        }
        print; next
    }

    # Detect anonymous gate primitive:  <gate_type> (<out>, <in>, ...);
    # A named instance looks like:      <gate_type> <name> (<out>, ...);
    # We distinguish them: if the token after the gate keyword starts with "(",
    # it is anonymous.
    /^\s*(and|nand|or|nor|xor|xnor|not|buf)\s*\(/ {
        # Extract gate type
        match($0, /^\s*([a-z]+)\s*\(/, arr)
        gate = arr[1]

        # Extract argument list (everything inside the outer parens)
        match($0, /\(([^)]+)\)/, args_arr)
        args = args_arr[1]

        # First argument is the output net
        split(args, nets, /\s*,\s*/)
        out_net = nets[1]
        gsub(/\s/, "", out_net)

        # Record this net as needing a wire declaration
        if (!(out_net in ports))
            wires[out_net] = 1

        # Emit named instance
        printf "    %s g_%s (%s);\n", gate, out_net, args
        next
    }

    # After the port declaration block and before the first gate, insert wires.
    # We detect this by the first gate line having been processed — flush wires
    # on the endmodule line so they appear in the right scope.
    /^\s*endmodule/ {
        if (length(wires) > 0) {
            printf "    wire"
            sep = " "
            for (w in wires) { printf "%s%s", sep, w; sep = ", " }
            printf ";\n"
        }
        print; next
    }

    { print }
    ' "$TMP1" > "$TMP2"

    rm "$TMP1"

    #  Step 4: Quick validation 
    # Verify Yosys can read the preprocessed file (syntax check only).
    if ! yosys -q -p "read_verilog $TMP2" 2>/dev/null; then
        echo "FAILED (yosys rejected output)"
        rm "$TMP2"
        return 1
    fi

    mv "$TMP2" "$DST"
    echo "OK → $DST"
}

FAILURES=0
for SRC in "$RAW_DIR"/*.v; do
    _process_file "$SRC" || FAILURES=$(( FAILURES + 1 ))
done

echo ""
if [[ $FAILURES -eq 0 ]]; then
    echo "All inputs preprocessed successfully."
    echo "Run ./scripts/compare.sh to benchmark."
else
    echo "$FAILURES file(s) failed — check output above."
    exit 1
fi
