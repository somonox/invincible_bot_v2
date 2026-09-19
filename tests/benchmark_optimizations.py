"""Compare fresh-process offline runs; require identical moves and game results.
Usage: python3 tests/benchmark_optimizations.py BASELINE GENERIC N1 NEON > result.json
Build each search_bench binary with the same example source before running.
"""
import json
import statistics
import subprocess
import sys

names = ["baseline", "optimized", "n1", "neon"]
binaries = dict(zip(names, sys.argv[1:], strict=True))
results = {name: [] for name in names}
reference = {}
for mode, games, moves, repeats in [("hybrid-residue", 4, 80, 3), ("hybrid", 2, 40, 1), ("combo-residue", 2, 40, 1), ("pc", 2, 40, 1)]:
    for repeat in range(repeats):
        order = names[repeat:] + names[:repeat]
        for name in order:
            run = subprocess.run([binaries[name], str(games), str(moves), "6", mode], capture_output=True, text=True, check=True)
            data = json.loads(run.stdout)
            signature = {key: data[key] for key in ("choice_hash", "attack", "largest_hit", "games", "decisions")}
            if mode not in reference:
                reference[mode] = signature
            assert signature == reference[mode], (name, mode, "behavior changed")
            results[name].append({"mode": mode, "repeat": repeat, "mean_ms": data["mean_ms"], "p95_ms": data["p95_ms"], "choice_hash": data["choice_hash"]})
summary = {name: {"mean_ms_median": statistics.median(r["mean_ms"] for r in runs if r["mode"] == "hybrid-residue"),
                  "p95_ms_median": statistics.median(r["p95_ms"] for r in runs if r["mode"] == "hybrid-residue")} for name, runs in results.items()}
print(json.dumps({"cpu": "Neoverse-N1", "depth": 6, "residue_decisions_per_run": 320,
                  "all_choices_and_results_identical": True, "summary": summary, "runs": results}, indent=2))
