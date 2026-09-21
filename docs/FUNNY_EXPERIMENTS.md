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

## Executable planning correction (after PC-B2B correction)

Baseline `923f1b5`: the beam allowed one-cell drops, while the adapter could only
send sonic soft drops. Live Funny logs repeatedly reported executable fallback
placements. This also made future unreachable spins look like viable reasons to
stack earlier. Funny now uses the sonic input graph at every depth, from the
configured spawn. Evaluation weights, beam width and survival priorities stay
the same. The geometry cache includes the movement model.

`funny_bench` accepts `executable` after the seed offset to validate each chosen
input path and simulate the adapter's existing one-ply fallback when necessary.
This flag is required for comparing actual playable continuations with the old
beam. These fixed-stream runs use the corrected PC-B2B rule, PC bonus 5 and 4x26.

| Arguments | Version | Unreachable plans | B2B breaks | B2B clears | Attack |
| --- | --- | --- | --- | --- | --- |
| `4 300 handheld 6 500 executable` | Before | 5 | 12 | 582 | 4584 |
| same | After | 0 | 16 | 581 | 4740 |
| `4 300 all 8 600 executable` | Before | 5 | 23 | 508 | 4305 |
| same | After | 0 | 23 | 493 | 4200 |

All eight games per version reached the 300-piece cap. This fixes invalid
planning, but does **not** establish improved B2B continuity or attack across
scenarios: breaks and attack remain mixed. No win-rate claim follows from these
solo simulations. Raw paired results are `benchmarks/funny-inputs-*.jsonl`.

The seed-500 fixture captures an S full spin at south/x=1/y=5 on rows
`[14,7,12,8,12,8,12,12]` (bottom first). The former generator includes it, but
the input pathfinder cannot reach it. A regression checks its exclusion and
Funny's executable choice at depths 1/3/6. Another checks all seven pieces at
both spawn heights in three spin modes, replaying every generated input path,
comparing reachable occupied cells/spin classes and alternating cache modes.

Validation: `cargo test --all-targets` passed on Windows. ARM64 native/thin-LTO
release passed 24 input-reachability, Funny, protocol and rule tests. Separate
ARM seeds `2 300 handheld 8 700 executable` reached 600/600 placements with zero
unreachable plans, mean total decision time 10.07/10.24 ms and p99 search time
13.21/13.00 ms. These are single-worker timings, not a concurrency/load test.

## Roof recovery evaluation

Baseline `2f347c3`. Funny now evaluates both the finished board and the time
spent carrying obstruction on the path to it. After each placement, let
`D = holes_count + cell_coveredness` (the latter counts occupied blocks above
an empty cell). Accumulate `E += D`. Also maintain outstanding cost `U`:
when a line clear reduces D, repay the corresponding fraction of the previous
U (`floor(U * D_after / D_before)`), then add D_after. Otherwise add D_after
without repayment. Rank field/B2B value minus `0.125 * E + 0.75 * U`.

A useful roof can be repaid by its later spin/PC. A spin alone is not proof of
recovery: residual obstruction retains cost, and the final field still receives
the ordinary quality evaluation. The smaller elapsed component survives
repayment to discourage postponing an otherwise identical recovery. Both costs
start at zero for each search; this is a five/six-placement preview heuristic,
not persistent cross-turn history, a ban on overhangs or a fixed stacking limit.
Search still uses a 64-node beam and keeps the best currently ranked history
per equal state; it does not preserve every cost-history tradeoff. Safety,
garbage receipt and B2B-break priorities precede these soft costs. The executable
fallback uses the corresponding one-placement cost. Normal and Expert do not
accumulate or compare these costs.

Matched deterministic comparisons (PC bonus 5, SRS-X, 4x26, executable inputs):

| Arguments | Version | B2B breaks | B2B clears | Attack | Obstruction sum | Height sum |
| --- | --- | --- | --- | --- | --- | --- |
| `4 300 handheld 6 800 executable` | Before | 10 | 609 | 4932 | 6560 | 6150 |
| same | After | 5 | 620 | 5220 | 4008 | 5475 |
| `4 300 all 8 900 executable` | Before | 24 | 501 | 4371 | 5516 | 6470 |
| same | After | 26 | 476 | 3982 | 4152 | 6389 |
| `4 300 all 0 1000 executable` | Before | 14 | 489 | 4417 | 7841 | 7842 |
| same | After | 14 | 523 | 4544 | 4613 | 6976 |
| `4 500 handheld 8 1100 executable` (separate seeds) | Before | 31 | 966 | 7605 | 12074 | 10677 |
| same | After | 34 | 953 | 7141 | 8625 | 10611 |

All 16 games per version reached their caps (5600 placements), with no
unreachable plans. Aggregate obstruction sum fell 33.1%, height sum fell 5.4%,
B2B breaks stayed at 79, and attack fell 2.1%. Strong garbage cases regressed
in attack/B2B breaks despite less obstruction, so this is a recovery/stacking
tradeoff, not an across-the-board strength or online win-rate improvement.
Maximum non-clearing streaks also did not consistently fall: useful setups
remain allowed. Raw results are `benchmarks/roof-{before,after}-*.jsonl`.

Regression fixture: rows `[7,7,3]`, current J, hold T, preview S/Z/I/O/T,
handheld spins. The selected T overhang creates holes, then executable J and S
spins remove the obstruction and leave a clean four-row tetris well. Tests also
verify that an intervening spin leaving obstruction only partially repays cost,
continued stacking increases it, and a recovered roof still retains the small
time cost. Existing PC, emergency downstack and buried-hole avoidance tests stay
in force.

Validation: Windows `cargo test --all-targets` passed. ARM native/thin-LTO
release passed the library, adapter, Funny, input-reachability and protocol
tests. Independent ARM seeds `2 300 handheld 8 1300 executable` reached their
caps with zero unreachable moves, mean decision times 10.76/10.56 ms and p99
search times 14.06/13.90 ms. This is a single-worker test, not a load benchmark.
