# Perfect-clear-first bot

The default static and meta search entry points now prioritize perfect clears.
The right GUI bot now switches between this objective and Combo; see
[HYBRID_POLICY.md](HYBRID_POLICY.md). The left uses `Objective::Combo`. Both show current combo and PC counts. The depth selector
defaults to 6 and the shared actual horizon remains limited to visible pieces.

## Decision rule

Search successive placement depths with the existing 64-state beam. At the first
depth where a surviving successor completes a PC, select a move leading to that
PC. Before a PC is found, rank candidates by board evaluation with the combo
feature set to zero. Setup moves that break a combo are allowed. If no PC is
found in the visible horizon, use the best surviving evaluated board.

The combo policy is available as `Objective::Combo`, used by the left GUI bot
and offline comparisons. No new model was trained.

This is a bounded search, not an exhaustive PC solver. It may miss solutions
discarded by the beam or outside the preview. An empty board at initialization
does not count as a PC: a move must clear at least one line and leave no cells.

## State, attack and GUI

`GameState` records `perfect_clears` and the one-move `last_perfect_clear` event.
Both simulation paths apply the existing 10-line PC bonus before cancellation.
The battle path previously omitted this bonus; it now matches single-player
search for PC detection and base attack. Paired turns still deliver new attacks
after both boards place.

The GUI shows round PC count, session PC count, placement count and a PC event
indicator. Highest-combo counters and the old-model weight comparison panel were
removed. Session totals increment once at placement, never on repaint.

## Historical offline comparison (before the SRS-X/spin update)

Same eight fixed 7-bag streams, 80 placements per game, empty 4-column board,
five preview pieces, requested depth 6, default weights, no incoming garbage:

| Policy | Placements | Perfect clears |
|---|---:|---:|
| Prior combo objective | 640 | 8 |
| PC-first objective | 640 | 182 |

The PC run had a 3.07 ms median and 12.27 ms p95 search time on the local machine.
These are local offline measurements, not online win-rate or latency guarantees.
The earlier three-cell-residue combo benchmark is a different scenario: its
occupied-cell count modulo four prevents a PC without incoming garbage.

Raw data: [combo](benchmarks/pc_comparison_combo.json),
[PC-first](benchmarks/pc_comparison_pc.json).

```sh
cargo test --all-targets
cargo run --release --example search_bench -- 8 80 6 combo
cargo run --release --example search_bench -- 8 80 6 pc
cargo run --release --bin battle-gui
```

All 19 tests pass. New coverage includes PC over extreme combo weights, a
two-piece PC requiring a combo break, short exhaustive PC oracles, matching PC
counts and attacks in both simulation paths, and excluding an initially empty
board from PC statistics. The GUI coordinator also verifies that repainting
does not increment PC totals.

Current SRS-X rules, limitations and verification are in [TETRIO_RULES.md](TETRIO_RULES.md).
