# Funny search experiments, 2026-09-21

Historical results: these experiments predate the PC-B2B rule correction.
Ordinary PCs were incorrectly counted as B2B breaks, so these results and
candidate rankings must be rechecked before drawing current-policy conclusions.

Baseline: `df4e3e5`. None of the experimental policies below was deployed.
The current 64-node beam, field evaluation and survival policy remain in use.

Use `funny_bench` with the arguments below. All comparisons use identical
7-bag streams, five visible previews, a 4x26 field and PC bonus 5. Generated
attack includes cancellation and the simulator's B2B surge approximation.

| Case | Arguments after `--` |
| --- | --- |
| Moderate garbage | `8 300 handheld 6 200` |
| Strong garbage, separate seeds | `4 600 handheld 8 300` |
| All spins, separate seeds | `4 600 all 0 300` |

| Policy | Moderate: breaks / attack | Strong: breaks / attack | All spins: breaks / attack |
| --- | --- | --- | --- |
| Baseline, width 64 | 41 / 7430 | 42 / 7748 | 44 / 7163 |
| Width 128 | 28 / 7833 | 49 / 7462 | 38 / 7475 |
| Width 64 with first-move quotas | 43 / 6669 | not run | not run |
| Width 64 with terminal continuation evaluation | 30 / 8052 | 51 / 7738 | not run |

Every completed run reached its piece cap. Wider search helped two scenarios
but regressed in strong garbage; it is not a consistent improvement. Forcing
first-move diversity also regressed. The quota experiment allowed at most one
quarter of retained nodes per first placement, filling unused slots by rank.

Terminal continuation evaluated one further known piece, or all seven possible
draws when the preview ended. It mixed mean and worst-case field quality with
penalties for risk and B2B breaks. This used no hidden future pieces, but it
did not infer remaining bag contents. Its strong-garbage result also regressed.

The width-128 experiment was timed separately on the ARM server with native
CPU flags and thin LTO (`2 300 handheld 8 400`): mean 20.8-21.3 ms per placement,
per-game p99 search latency 25.9-26.8 ms. This is a single-worker benchmark,
not a 20-room load test. `funny_bench` now reports p95 and p99 search latency
alongside mean time so future changes can be checked for stalls.

Next investigations should distinguish prediction errors from search pruning:
reproduce real garbage arrival/cancellation from replays, and infer possible
remaining 7-bag pieces from observed history before assigning continuation
probabilities. These are proposed investigations, not implemented improvements.
