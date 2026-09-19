# Combo vs PC GUI and SRS-X rules

The left GUI bot uses `Objective::Combo`: preserve the uninterrupted clear chain,
then favor more clearing placements, then board quality. The right now uses [adaptive PC/Combo](HYBRID_POLICY.md); its PC branch selects
the earliest PC discovered within the visible preview. Equal search depth, paired commit timing, preview-only search and the
64-state beam are retained. Attack is a tie-breaker after the objective/quality.
Neither bot is guaranteed an exhaustive solution or a particular win rate.

Both boards show current combo, PC counts, last spin and attack. Internally combo
counts consecutive clearing placements; display follows TETR.IO's convention:
first clear = 0, second consecutive clear = 1. A non-clear resets it. The online
adapter converts protocol -1/0/1 into internal 0/1/2 without losing a clear.

## Rotation and spin

SRS-X uses ordered 90- and 180-degree kick tests. Coordinates are converted from
reference Y-down to this engine's Y-up. I orientations now rotate about the
same half-cell pivot as their kick table, correcting the prior mirrored Y offsets.
A successful rotation records its spin classification. A successful translation
or nonzero drop clears it; a zero-distance hard drop retains it. Move generation
keeps both spin and ordinary paths to the same occupied cells when applicable.

The GUI defaults to `All`, with selectable `All-Mini+` and `T-spins`:

- T: grounded, three occupied pivot corners; two occupied front corners give a
  full spin. Otherwise mini, except matching Fin/TST quarter-turn kicks promote
  to full. This promotion does not apply to arbitrary 180-degree kicks.
- All: other pieces must be immobile in all four translation directions after
  rotation, then receive full spin damage. Ordinary T detection uses corners.
- All-Mini+: immobile pieces receive mini credit, with regular full T-spins
  retaining full credit. This also allows immobile T mini credit without corners.
- T-spins: only the T corner test is credited.

Full spin single/double/triple/quad base attack is 2/4/6/10. Mini single/double/
triple is 0/1/2. Spins that clear lines maintain B2B, just like quads. B2B chaining
uses the logarithmic formula, followed by multiplier combo scaling and integer
flooring. The first eligible clear starts B2B without receiving a continuation
bonus. Search and battle share this calculation. The GUI keeps its +10 PC bonus; online search reads the room PC bonus, including zero.

The adapter reads `spins` from room config. In addition to the GUI modes, online
play supports handheld corner spins (T/L/J/S/Z; non-T base attack is halved),
all-mini, all+, T-spins+, mini-only and none. Unknown modes conservatively receive
no predicted spin credit instead of stopping play. Nonstandard `stupid` spins
use the grounded rotation test; credit on non-rotation placements is approximate.
Multiplier, classic guideline, modern guideline and no-combo attack tables are
selected from room config. `!setup` only changes width, height and kickset.

The adapter Its pathfinder
uses the same SRS-X rotation/spin helper, includes rotate180, and treats softDrop
as a sonic drop instead of collapsing arbitrary one-cell paths. If a selected
placement cannot be expressed by these inputs, it selects an executable fallback
and logs the actual fallback move. Other kicksets remain unsupported.

## Scope and sources

This implements the selected rotation/spin/attack profile, **not every TETR.IO
room setting or frame-level parity**. Existing local opener cancellation,
queued-garbage timing and Surge charge are approximations retained from the
simulator. B2B charging options, garbage-blocking variants, garbage multipliers,
lock-delay resets and real network input timing are not
fully modeled. GUI animations interpolate toward a validated move; they are not
an exact replay of all input frames. No live login or online match was performed.

Reference inspected 2026-09-16:

- [Official TETR.IO public client](https://tetr.io/js/tetrio.js): SRS-X table.
  Download SHA-256: `eed14d6f268b2d6088dd82bc869a19f98410097e2c14a1f43cecb5a3d878526f`.
- [Triangle source](https://github.com/halp1/triangle), pinned `@haelp/teto` 4.2.7:
  `src/engine/utils/kicks/data.ts`, `src/engine/utils/tetromino/data.ts`,
  `src/engine/utils/tetromino/index.ts`, `src/engine/index.ts`,
  `src/engine/utils/damageCalc/index.ts`, and `src/utils/adapters/core/index.ts`.
  This is the project's existing third-party integration, not official TETR.IO
  documentation. The captured SRS-X fixture is attributed under its
  [MIT license](../tests/fixtures/TETO_LICENSE.md).

## Validation

`cargo test --all-targets`: 27 tests, including all 24 ordered common/I rotation
transitions against the captured fixture, I pivot geometry, T mini/full tests,
non-T immobility, spin damage/B2B/combo vectors, replayed executable spin paths,
protocol counters, and the GUI worker's left/right objective assignment. Existing
PC, combo, preview and synchronized-turn regressions remain covered.

`cargo build --release --bins --example search_bench` builds the GUI and adapter.

Latest offline smoke benchmark: four identical fixed 7-bag streams, 80 placements
per stream, empty 4-column boards, depth 6, no incoming garbage. Combo cleared on
304/320 placements with 14 chain breaks and 4 PCs; PC cleared on 226/320 placements
with 78 chain breaks and 90 PCs. Median search was 7.95 ms (combo), 0.95 ms (PC);
p95 was 9.16/6.88 ms. These demonstrate different objectives, not online win rates.
Raw results: [combo](benchmarks/srs_x_combo.json), [PC](benchmarks/srs_x_pc.json).
The benchmark's `max_combo` field still counts consecutive clears; subtract one
for the GUI's displayed TETR.IO combo convention.
