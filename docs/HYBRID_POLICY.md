# Garbage-aware PC / combo policy

The GUI left bot keeps the Combo objective. The right bot and online adapter use
`find_hybrid_move`. There is no opponent combo threshold or threshold slider.
The GUI reports the selected strategy and predicted cancellation. Shared depth,
paired turns, current combo display, SRS-X and spin settings are preserved.

Every placement is planned again from the latest actual board and incoming queue:

1. Keep the occupied-cell divisibility check: cells must be divisible by
   gcd(width, 4) to pursue PC. On a four-wide board, pieces and clears cannot
   change that remainder. Incoming garbage does not bypass this condition;
   reconsider it after garbage actually changes the board.
2. With incoming garbage, search a full visible horizon. Prefer fewer received
   lines first, then a smaller sum of outstanding garbage after each placement
   (earlier cancellation). With equal defensive outcomes, prefer PC when the
   residue allows it; otherwise preserve the clear chain. An immediate PC is
   allowed when it cancels efficiently. A multi-piece PC setup can lose to an
   immediate combo clear when that prevents garbage from rising.
3. With an empty garbage queue, probe for a reachable PC. If none is discovered
   in the bounded beam/preview, use Combo. Pure PC and Combo APIs remain available.

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
