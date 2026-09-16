# Combo search improvements — 2026-09-16

This records the earlier combo-focused implementation and measurements. The current default has since changed to [PC-first](PERFECT_CLEAR.md); both GUI bots now use that objective.

## Changes

- Replace recursive per-node pruning with a global 64-state beam at each depth.
- Rank non-terminal candidates by uninterrupted clears, total clear moves, then board evaluation. Do not add an arbitrarily large combo reward to a finite death penalty.
- Search only the visible preview. A local game's hidden bag is excluded too. Empty hold consumption is accounted for in the common search horizon; an exhausted preview is a leaf, not a top-out.
- Continue searching after perfect clears.
- Deduplicate equivalent piece placements and equivalent successor states. State equality includes the full combo count, preview, garbage and B2B state.
- Evaluate each distinct successor once before sorting.
- Skip empty sky in move generation. Expand visitation storage to cover every indexed state and negative piece pivots instead of clamping coordinates or silently dropping states at 1,024 queue entries.
- Initialize battle GUI's meta network with the deterministic baseline rather than random weights.

These changes apply to `find_best_move` and `find_best_move_meta`. The older `find_best_move_original` remains the reference opponent in battle GUI; it is not the baseline binary used for the measurements below.

## Local comparison

Baseline: repository commit `6fcd29fb9bf2725644dc0d7c0d927ca151966ee1`, built before edits, using the same benchmark harness. Windows, AMD Ryzen AI 7 350, Rust 1.94.0, release builds. Eight fixed 7-bag streams of 80 placements each, 4-column board with three-cell starting residue, five visible next pieces, requested depth 6, zero/default meta network, no incoming garbage.

| Metric | Before | After |
|---|---:|---:|
| Placements | 640 | 640 |
| Moves clearing at least one line | 621 | 628 |
| Combo breaks | 17 | 12 |
| Median search time | 305.26 ms | 16.87 ms |
| 95th percentile search time | 1,289.65 ms | 26.15 ms |
| Mean search time | 427.88 ms | 17.68 ms |

Raw per-game measurements: [before](benchmarks/before.json), [after](benchmarks/after.json).

This is one local before/after sample, not an online win-rate claim or a guaranteed latency bound. The baseline invents random hidden pieces, so its results can vary even with the same external piece streams. The new version reproduced the same per-game outcomes on two runs. Timing also varies with machine load; these measurements were not made on an isolated benchmark host. Some individual games still have shorter maximum combos than the baseline.

## Reproduce

```sh
cargo test --all-targets
cargo build --release --bins
cargo run --release --example search_bench -- 8 80 6 combo-residue
```

To compare the old code again, check out the baseline commit in a separate worktree, copy `examples/search_bench.rs` into that worktree's `examples` directory, and run the same command there. Run the two benchmarks sequentially.

Seven regression tests cover full-spawn reference BFS versus optimized move generation (4- and 10-column boards, all seven pieces), duplicate placements, preview determinism and exhaustion, empty hold, local bag isolation, perfect-clear continuation, static/meta evaluator agreement, and a depth-3 exhaustive continuation oracle across 28 positions. All targets build and tests pass. A local stdin/stdout smoke check also confirmed that the release adapter returns identical key sequences for repeated identical states.

## Remaining limits

### Synchronized battle GUI

The depth selector now applies to both algorithms. At the beginning of a turn,
both searches receive snapshots of the same pre-turn situation and a shared
depth limit, capped by both visible previews (including empty-hold consumption).
Changing the selector affects the next turn, never one half of an in-flight pair.
The UI displays the effective depth and each bot's last search duration.

Both results must arrive and both animations must finish before either board
locks a piece. One shared turns-per-second limit controls the pair. Garbage is
resolved against each player's pre-turn queues; newly sent attacks are delivered
only after both placements, avoiding a systematic processing-order advantage.
This mode compares decisions at equal placement pace, not real-time search speed.

Five GUI coordinator tests and two simultaneous-garbage tests were added; all
14 tests pass with `cargo test --all-targets`.

The beam is approximate and only uses known upcoming pieces; unknown-bag probability modeling and wider tactical search remain possible follow-up work. Online TETR.IO execution, input timing, garbage-rule parity and competitive win rate have not been validated by this offline test. No model training or external account login was performed.
