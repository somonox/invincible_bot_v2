# Garbage-aware PC / combo policy

The GUI left bot keeps the Combo objective. The right bot and online adapter use
`find_hybrid_move`. There is no opponent combo threshold or threshold slider.
The GUI reports strategy, predicted cancellation and attack. Shared depth,
paired turns, current combo display, SRS-X and spin settings are preserved.

Every placement is planned again from the latest actual board and incoming queue:

For the current small-residue combo policy, also see [the finite-state combo
solver](COMBO_SOLVER.md). After the beam below, a covered compact field with no
queued garbage and no selected PC uses the table solver's root move. The beam
is re-run with that root fixed to report its attack diagnostics. Tall-field
multiplier planning and incoming-queue defense keep the ranking below. The left
GUI uses the table directly when eligible. Table decisions use the full visible
preview independently of the beam-depth control.

1. Keep the occupied-cell divisibility check: cells must be divisible by
   gcd(width, 4) to pursue PC. On a four-wide board, pieces and clears cannot
   change that remainder. Incoming garbage does not bypass this condition;
   reconsider it after garbage actually changes the board.
2. Search a common full preview horizon. With incoming garbage, fewer received
   lines remain the first priority. Among equally safe paths, maximize total
   generated attack, including combo multipliers, spins, B2B and PC bonuses.
   Early cancellation is now a tie-breaker, so safe line blocking can buy time
   for a larger multiplied hit. There is no combo count that triggers cash-out.
3. With an empty queue, also compare full-horizon damage instead of returning the
   first discovered PC. A PC competes through its actual attack bonus. Equal
   damage prefers earlier cancellation, sustained clearing, PC (when residue
   allows it), then board quality. Pure PC and pure Combo APIs remain unchanged.

The 64-node beam reserves half its slots for damage, a quarter for clear-chain
continuation and a quarter for PC/board setups. These are search budgets rather
than strategic thresholds. This prevents small early clears from being pruned
before a later spin/quad pays off. Final ranking uses attack actually generated
within the visible horizon, with no reward for imagined future pieces or stacking
height by itself. The GUI and adapter report total preview attack and largest hit.

Both pending and future packets can be canceled. A non-clearing placement only
receives ready garbage, up to the cap. Received lines and canceled lines are
tracked separately: consuming the queue by taking damage never earns cancellation
credit. Unlike the PC-only objective, defense does not stop at the first PC;
remaining garbage and the following clear-chain break still affect its choice.

## Online timing

`garbage-context.ts` snapshots packet amount and `packet.frame + garbage.speed`,
the live garbage cap, configured PPS and previous input-path duration immediately
before each `play`. The adapter uses relative frame deadlines. The current lock
uses the previous path duration (12 frames before the first path); subsequent
locks include the wrapper's PPS wait plus estimated input duration. Each new
snapshot replaces this prediction. The standard protocol queue remains a
conservative fallback when timing data is unavailable. Fractional packet amounts
are rounded up instead of being silently dropped.

The GUI keeps its existing pending/one-turn queued delivery model. Search uses
conservative one-for-one cancellation; GUI opener/B2B cancellation bonuses and
online room-specific attack multipliers are not fully modeled. Future garbage
holes use the last known hole, and attacks not yet present in the queue are not
predicted. These limits mean this is not a guarantee of surviving a faster player.

## Verification

Regression fixtures cover a three-placement PC setup that switches to an
immediate two-line cancellation under an eight-line ready queue, immediate PC
cancellation, delayed packets allowing a PC before arrival, arrival deadlines and
caps, queued-only pressure, zero-attack line blocking, residue preservation,
empty-queue recovery, protocol fallback and independent TypeScript snapshots.
Existing search, SRS-X, PC and paired GUI tests remain required.

The JSON files under `docs/benchmarks/hybrid_*` are historical measurements from
the old combo-threshold policy at commit `dfe324d`; they are not measurements of
this garbage-aware policy. No live-match win-rate improvement is claimed.

Multiplier regression tests compare against exhaustive depth-three enumeration.
On rows `[7,7,7,7,3,1]`, T current, I hold and O/I/S preview, internal combo 9,
immediate maximum attack gives at most 17 total lines over three placements;
small clears followed by a multiplied quad yield 19. With eight incoming lines,
the latter path still receives zero garbage. The same investment preference is
verified at internal combos 0, 2 and 5, while a one-placement horizon spends the
quad immediately. These are engine fixtures, not live-match win-rate evidence.

A fixed-stream depth-six check (four seeds, 80 placements each, three-cell
residue, no incoming attacks) completed all 320 placements for both policies:

| Policy | Generated attack | Clear-chain breaks | Mean search time |
| --- | ---: | ---: | ---: |
| Multiplier hybrid | 1486 | 13 | 23.1 ms |
| Pure Combo control | 1432 | 6 | 22.0 ms |

The hybrid produced 3.8% more attack in this sample while breaking chains more
often; maximizing preview damage does not guarantee the longest combo or better
win rate. The benchmark now preserves B2B between placements, so compare these
new runs to each other rather than the older JSON files. Reproduce with
`cargo run --release --example search_bench -- 4 80 6 hybrid-residue` and
`cargo run --release --example search_bench -- 4 80 6 combo-residue`.
Raw runs: [hybrid](benchmarks/multiplier_residue.json),
[control](benchmarks/multiplier_combo_control.json).
