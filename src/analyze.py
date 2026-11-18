#!/usr/bin/env python3
"""Summarize the CSVs written by `compare` (default: output/stats/).

    python3 scripts/analyze.py [--dir output/stats] [--plot]

`--plot` writes PNGs to <dir>/plots/ and needs matplotlib.
"""
import argparse
import csv
from pathlib import Path
from statistics import geometric_mean


def load(path):
    if not path.exists():
        return []
    with path.open() as f:
        return list(csv.DictReader(f))


def num(row, key):
    try:
        return float(row[key])
    except (KeyError, TypeError, ValueError):
        return None


def pairs(rows, key):
    """(circuit, value) for rows with a positive numeric `key`."""
    return [(r["circuit"], v) for r in rows if (v := num(r, key)) and v > 0]


def speedup_line(title, data):
    if not data:
        return
    lo = min(data, key=lambda p: p[1])
    hi = max(data, key=lambda p: p[1])
    gm = geometric_mean(v for _, v in data)
    print(f"{title:<16} geomean {gm:.2f}x   min {lo[1]:.2f}x ({lo[0]})   max {hi[1]:.2f}x ({hi[0]})")


def stage_shares(stages):
    """Mean percent of pipeline time per stage, in pipeline order."""
    by_stage = {}
    for r in stages:
        if (v := num(r, "pct_of_total")) is not None:
            by_stage.setdefault(r["stage"], []).append(v)
    return {s: sum(v) / len(v) for s, v in by_stage.items()}


def summarize(pipe, stages, rename):
    print(f"{len(pipe)} circuits\n")
    speedup_line("pipeline x-net", pairs(pipe, "speedup_net"))
    speedup_line("pipeline x-raw", pairs(pipe, "speedup_raw"))
    speedup_line("rename", pairs(rename, "yosys_over_ours"))

    slowest = sorted(pairs(pipe, "ours_median_us"), key=lambda p: -p[1])[:3]
    print("\nslowest (ours, us): " + ", ".join(f"{c} {v:.0f}" for c, v in slowest))

    print("\nmean share of pipeline time:")
    for stage, pct in stage_shares(stages).items():
        print(f"  {stage:<8} {pct:5.1f}%  {'#' * round(pct / 2)}")


def memory(rows):
    total = [(r["circuit"], num(r, "peak_bytes")) for r in rows if r["stage"] == "total"]
    top = sorted((p for p in total if p[1]), key=lambda p: -p[1])[:3]
    print("\npeak heap (KiB): " + ", ".join(f"{c} {v / 1024:.0f}" for c, v in top))


def plot(out, pipe, stages):
    import matplotlib.pyplot as plt

    out.mkdir(parents=True, exist_ok=True)
    data = pairs(pipe, "speedup_net")
    if data:
        fig, ax = plt.subplots(figsize=(10, 4))
        ax.bar(*zip(*data))
        ax.axhline(1, color="k", lw=0.8)
        ax.set(yscale="log", ylabel="Yosys / ours (x-net)")
        ax.tick_params(axis="x", rotation=60)
        fig.tight_layout()
        fig.savefig(out / "speedup.png", dpi=150)

    circuits = [r["circuit"] for r in pipe]
    fig, ax = plt.subplots(figsize=(10, 4))
    bottom = [0.0] * len(circuits)
    for stage in stage_shares(stages):
        vals = {r["circuit"]: num(r, "pct_of_total") or 0 for r in stages if r["stage"] == stage}
        heights = [vals.get(c, 0) for c in circuits]
        ax.bar(circuits, heights, bottom=bottom, label=stage)
        bottom = [b + h for b, h in zip(bottom, heights)]
    ax.set(ylabel="% of pipeline time")
    ax.legend(ncol=6, fontsize=8)
    ax.tick_params(axis="x", rotation=60)
    fig.tight_layout()
    fig.savefig(out / "stage_share.png", dpi=150)
    print(f"\nplots written to {out}")


def main():
    ap = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    ap.add_argument("--dir", type=Path, default=Path("output/stats"))
    ap.add_argument("--plot", action="store_true")
    args = ap.parse_args()

    pipe = load(args.dir / "compare.csv")
    if not pipe:
        raise SystemExit(f"no compare.csv in {args.dir}; run `make regress` first")
    stages = load(args.dir / "compare_stages.csv")
    summarize(pipe, stages, load(args.dir / "compare_rename.csv"))
    if mem := load(args.dir / "memory.csv"):
        memory(mem)
    if args.plot:
        plot(args.dir / "plots", pipe, stages)


if __name__ == "__main__":
    main()
