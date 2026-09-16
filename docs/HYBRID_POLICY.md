# Adaptive PC / combo opponent

The GUI left bot stays on the existing Combo objective. The right uses
`find_hybrid_move`, with a visible strategy reason and a configurable opponent
combo threshold (default 20, range 1–20). This is an initial tuning value, not an
empirically optimized win-rate threshold. Shared depth and paired turns remain.
Changes to the threshold take effect on the next search, not an in-flight pair.

The decision is recomputed on every actual board snapshot:

1. If occupied cells are not divisible by gcd(width,4), prefer Combo immediately.
   In a 4-column board, each piece adds 4 cells and each line removes 4; the
   remainder cannot change until garbage actually enters the board. Pending or
   speculative future garbage is not treated as permission to chase an impossible
   PC. The board is reconsidered after it changes.
2. If the opponent's displayed combo is at least the threshold, use the full
   Combo objective. Even an immediate PC is not specially prioritized: clearing
   the residue can interrupt the next few clearing moves. The combo search can
   still choose a PC naturally if that is its best continuation.
3. Otherwise probe for a PC in the bounded visible preview. Use that plan only
   if a complete PC was actually found, reporting its placement depth.
4. If the probe fails, rerun with Combo. Do not return the pure PC evaluator's
   non-PC board-quality fallback. Failure means not found within the beam and
   preview, not a proof of global impossibility.

This changes the GUI right bot. Pure PC and Combo entry points remain available
for comparisons, and the online adapter remains on its previous pure PC policy.
No model training or live TETR.IO matches were performed.

## Verification

`cargo test --all-targets`: 34 tests pass. New coverage checks residue rejection
including pending garbage, low/high opponent threshold boundaries, two-piece PC
setups, failed-PC fallback, full-horizon Combo even with an immediate PC,
reconsideration after garbage/opponent changes, width-dependent divisibility,
terminal states, and GUI left/right worker wiring.

The recorded offline checks below used the previous threshold of 6. The GUI
default is now 20; these are historical results, not measurements of that new default.
Offline fixed-stream checks use four seeds, 80 placements each, depth 6 and no
incoming garbage. Three-cell residue results:

| Policy | Clearing moves / 320 | Chain breaks | PCs |
| --- | ---: | ---: | ---: |
| Pure PC | 304 | 13 | 0 |
| Hybrid | 313 | 7 | 0 |
| Pure Combo | 313 | 7 | 0 |

On empty boards against a fixed displayed opponent combo of 0, hybrid completed
91 PCs, using PC plans for 312 moves and Combo fallback for 8. Against fixed
opponent combo 8, it uses the Combo policy for all 320 decisions. These are
controlled policy checks, not head-to-head win-rate estimates. Existing local
garbage/Surge approximations still apply to GUI battles.

Raw results and reproduction:

- [hybrid residue](benchmarks/hybrid_residue.json): `cargo run --release --example search_bench -- 4 80 6 hybrid-residue 0 6`
- [PC residue](benchmarks/pc_residue.json): `cargo run --release --example search_bench -- 4 80 6 pc-residue 0`
- [Combo residue](benchmarks/combo_residue.json): `cargo run --release --example search_bench -- 4 80 6 combo-residue 0`
- [low opponent combo](benchmarks/hybrid_low_combo.json): `cargo run --release --example search_bench -- 4 80 6 hybrid 0 6`
- [high opponent combo](benchmarks/hybrid_high_combo.json): `cargo run --release --example search_bench -- 4 80 6 hybrid 8 6`
