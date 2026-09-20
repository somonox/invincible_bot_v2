# Finite-state combo solver

The combo policy now has a separate solver in `src/rl/combo_solver.rs`. It does
not use the height/holes/PC feature weights and does not change the physics,
SRS-X kicks, spin attribution, or attack rules.

## State graph and offline calculation

Start with every bottom-two-row configuration containing at most six cells and
no complete row. Enumerate actual SRS-X placements and retain only placements
that clear at least one line. Repeatedly add their resulting boards until the
graph is closed. The resulting graph contains **1,026 boards and 5,882 distinct
piece-to-board transitions**. All reached boards fit the eight-row encoding;
the builder fails if any transition escapes it instead of silently truncating.
This is a reachable component, not every possible six-cell field.

Each offline state is `(board, hold including empty, remaining 7-bag mask)`.
Bellman dynamic programming enumerates every possible next draw, placement and
hold decision for 128 clearing placements. Failure to clear terminates the
chain. An empty hold consumes another draw but still only one placement, and
does not allow a second hold on that turn. The value is expected chain length
capped at 128, not damage or a hand-tuned board score. The offline policy sees
the exact remaining bag mask and the drawn piece, but no preview.

The reproducible binary `data/combo-table.bin` is 2,131,496 bytes. It contains
the graph and values quantized down in units of 1/256 of a placement. It is
embedded in each executable and decoded once per process. It needs no network,
training service, runtime file path, or runtime table generation.

## Online decision

The solver enumerates **all clearing paths through the supplied preview**, up
to five next pieces, with memoization over board, hold and preview position.
It uses the offline table at the end of the preview and prefers higher predicted
continuation, then actual immediate attack when tied. No hidden local bag is
read and no random future sequence is sampled.

Bag boundaries are not assumed: all bag remainders compatible with the visible
sequence are retained, and the minimum table value across them ranks terminal
boards. This is a conservative ranking across possible phases, **not a proof
of an achievable expected length under hidden phase information**: the offline
policies have exact bag-mask information. It also assumes a 7-bag randomizer;
an inconsistent visible sequence falls back. Future preview revelation is not
modeled by the offline value and can improve actual play substantially.

Integration:

- GUI left / `Objective::Combo`: use the table whenever the current board is
  covered, no incoming garbage exists, and a complete visible clear chain exists.
- Online rooms start in Normal mode (the original beam). `!expert` toggles
  the table-enabled Expert mode; `!expert on` / `!expert off` explicitly select
  it. The setting is scoped to the room worker and updates on the next decision.
  GUI policies retain their table-enabled behavior.
- Right GUI / Expert online hybrid: keep an already selected PC and all queued-garbage
  defense. In a quiet combo phase, use the table's root move. Re-run the existing
  beam with that root fixed to obtain attack/cancel/PC diagnostics for that move.
- A current board outside the table uses the original beam. In particular, the
  bot does **not** cash out a tall stack early just to enter the table: the
  multiplier investment regression remains intact.
- Unsupported width/height, unknown current, longer-than-supported previews,
  or a missing complete chain use the original beam. Spawn heights 20 and 26
  are checked against real move generation with multiple spin modes.

The GUI depth setting controls the beam, including hybrid diagnostics and
fallbacks. The table policy always uses the entire supported visible preview.
Its 128-placement offline horizon is separate from that setting.

## Infinite-chain analysis

The builder also computes a greatest fixed point for **one next-piece preview,
full hold, exact 7-bag phase and no incoming garbage**. For every possible next
draw, at least one legal clearing action must remain inside the surviving set.
In this generated component, **zero states survive**. Closed-loop existence
alone is therefore not sufficient to guarantee survival against every draw.

This result does NOT prove impossibility with five previews, larger residues,
other starting components, or different rules. The shipped solver is a
long-continuation policy, not a certified infinite-combo strategy.

## Reproduction

```sh
cargo run --release --no-default-features --example build_combo_table -- target/combo-table.bin
# Compare target/combo-table.bin against data/combo-table.bin before replacing.
cargo test --locked --all-targets
cargo run --release --no-default-features --example combo_bench -- 100 1000 3 0
cargo run --release --no-default-features --example combo_bench -- 100 2000 6 0
cargo run --release --no-default-features --example combo_bench -- 100 10000 6 1000
cargo run --release --no-default-features --example combo_bench -- 100 2000 3 1000
```

Benchmark arguments are games, placement cap, initial residue (3 or 6), seed
offset. Each pair uses the identical fixed 7-bag stream, five previews, empty
initial hold, SRS-X, a 4x26 board, no incoming garbage, and stops at the FIRST
non-clearing move. The original depth-six beam with default weights is the
control. Reported length counts consecutive clearing placements, not the UI's
combo index. Zero-length results and runs reaching the cap are included.

Windows release measurements on 2026-09-20:

| Seed offset | Residue | Games / cap | Beam mean | Table mean | Table median | Table reached cap |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| 0 | 3 | 100 / 1,000 | 63.92 | 222.46 | 141 | 3 |
| 0 | 6 | 100 / 2,000 | 19.51 | 911.43 | 897 | 27 |
| 1,000 (held out) | 6 | 100 / 10,000 | 20.32 | 1,394.66 | 505 | 1 |
| 1,000 (held out) | 3 | 100 / 2,000 | 59.82 | 250.35 | 97 | 1 |

For the held-out six-cell run, table + fallback averaged 0.056 ms per decision
(p99 0.140 ms), versus 7.28 ms for the beam on this host. These are offline
combo-policy timings, not total hybrid/online latency. The hybrid still runs its
attack/defense beam. Capped observations understate the uncapped mean; these
results do not establish an infinite chain, live win rate, or faster-PPS defense.
