# Public bot service policy

The online bot requires SRS-X. It spectates and lists incompatible
settings. Give the bot host and issue `!setup` to apply the required settings.
After a successful `!setup`, the bot returns host to the person who gave it host.
If that person has left or no previous owner was observed, it returns host to
the command sender. Joining or transferring host alone never changes room options. The actual round engine is
checked again before starting the Rust adapter, which also rejects other kicks.

`!setup` changes exactly three options: `boardwidth=4`, `boardheight=26`, and
`kickset=SRS-X`. PC bonus, spin mode, combo table, garbage blocking, handling and
other room options are preserved. PC 5 and `handheld` are allowed without any
extra setup. The adapter uses the configured PC attack and combo table, and
supports handheld corner spins with half non-T spin damage.

The current input executor still needs hold, 180 rotation and hard drop enabled,
ARR 0 and SDF 41 to execute its paths. These are checked at round start with an
explicit message when missing; `!setup` does not silently overwrite them.
Other prediction limits, including garbage blocking/multiplier and B2B charging
variants, remain approximations described in [rule limitations](TETRIO_RULES.md).

## Room commands and capacity

- `!help` lists commands, mode behavior and control permissions in English.
  Anyone in the room can use it. `!bot` reports the current status and mode.
- `!funny` toggles **Funny** mode; `!funny on` / `!funny off` explicitly set it.
  It favors B2B preservation and growth through eligible spin clears / tetrises,
  permitting non-clearing setup moves. Safety comes first: field height, buried
  holes (converted to recovery rows) and pending garbage (up to one rise cap)
  reserve eight rows below the configured visible ceiling. Search minimizes
  excess pressure throughout the path, then at its end, then received garbage.
  Within that safe space, fewer B2B breaks wins, then a higher final B2B level,
  board quality and attack. Dangerous stacking yields to ordinary clears even
  without an opponent's attack; safe spin clears / tetrises still preserve B2B.
  Combo and fixed PC feature rewards do not drive this mode. This is a bounded
  preview heuristic, not a guarantee against topping out.
  Funny and Expert are mutually exclusive: enabling one disables the other;
  toggling the active mode off returns to Normal. Explicitly disabling an
  inactive mode leaves the current mode alone. Permissions, per-room lifetime,
  next-decision updates and PPS cap are the same as Expert.
  `data.funnyMode` is a strict boolean; if a client supplies both flags as true,
  Funny takes precedence and the adapter reports Expert as false. The actual
  executable fallback also prefers preserving/growing B2B after safety.
- Rooms start in **Normal** mode. `!expert` toggles **Expert** mode; `!expert on`
  and `!expert off` explicitly set it. Only the room host or original inviter
  can change it. Expert prioritizes uninterrupted clears using the table when
  available and combo-first beam search for setup, uncovered boards or incoming
  garbage. It does not revert to PC/attack ranking when the table is unavailable.
  Predicted garbage received is minimized before comparing chain length; PC and
  B2B attack never outrank a longer equally safe chain. Normal keeps the PC/attack
  policy. PPS limits stay intact.
  Changes apply at the next planning decision without interrupting current
  inputs. The setting is per room worker, survives subsequent rounds, and resets
  to Normal when the bot leaves/rejoins or the service restarts. `!bot` shows it.
  Each state snapshot carries a strict boolean `data.expertMode`; a missing or
  invalid value means Normal, so older clients do not opt in accidentally.
- `!pps <0.1-5>` sets speed; default 2. Invalid/partial numbers and values over 5
  are rejected. Runtime also clamps speed and spaces consecutive hard-drop inputs
  by at least 60/PPS frames, preventing catch-up bursts after lag or speed changes.
- `!setup` applies the required room settings in the lobby. The bot must hold
  host permission. The original inviter or the person who gave the bot host can
  run it. Settings must pass validation before host is returned. A failed update
  keeps host with the bot so setup can be retried. Unsupported settings continue
  to block play until fixed.
- `!leave` removes the worker from the room.
- `!pps` and `!leave` require the room host or original inviter.
- `!bot` displays status and help, including actual incompatible values and
  required replacements. Room status changes and blocked rounds are also logged.
- The pool defaults to 20 rooms, at most two per inviter, with
  duplicate room invitations ignored. Reservations are released on exit/failure.
- Each worker searches independently, as before; there is no shared search queue
  or three-search concurrency limit. The room pool still defaults to 20 workers.
- The worker leaves as soon as no other room members remain, stops its adapter
  and saves a partial replay. Spectators count as occupants. Occupied rooms have
  no inactivity timeout; `BOT_IDLE_MINUTES` is no longer used.
- Connection, adapter initialization and individual moves have
  deadlines. Failed rounds spectate until the next match; owned child processes
  are stopped and escalated to SIGKILL after 500 ms if necessary. A stale result
  from an earlier round cannot stop its replacement.

## Replay storage

The SDK ReplayManager exports native TETR.IO versus JSON to `replays/*.ttrm`.
Opponent frame streams are subscribed without simulating each opponent engine.
The bot's own transmitted `game.replay` packets are separately captured through
`client.ribbon.send` and piped into the same ReplayManager. Incoming event listeners
do not receive outgoing gameplay. This records the bot's start/full state, actual
key events and IGEs without re-sending packets or copying planned future inputs.
A full match is saved on match end, not when the bot alone tops out. Interrupted
matches are marked `.partial.ttrm`. Joining announces that games are recorded.
The files stay local and are excluded from git; no replay is published or sent.

Replays have no application-imposed age, file-count, total-size, per-match-size
or pending-write quota. There is no automatic replay deletion or size-triggered
recording cutoff. Previous `BOT_REPLAY_*` retention variables are no longer used.
Files are written atomically with private permissions and unique names. Saving
still reports filesystem/write failures. Shutdown waits for queued writes.
Restart/crash before save can lose an in-progress match. Native playback in the
TETR.IO UI still needs a live-match check; tests validate saved JSON and lifecycle.

| Environment variable | Default | Allowed range |
| --- | --- | --- |
| `BOT_MAX_WORKERS` | 20 | 1-32 |
| `BOT_LOG_MOVES` | off | `1` enables per-move diagnostics |

Set overrides in `tetrio-bot/.env` and restart to apply them. PPS 5 is a hard
service ceiling, not an environment override. See [hosting commands](HOSTING.md).

## Verification

`cd tetrio-bot && bun node_modules/typescript/bin/tsc --noEmit && bun test`
checks typing, settings/host transitions, speed/command permissions, independent
concurrent searches, process failures, stale rounds, immediate empty-room cleanup, replay
lifecycle (including outgoing self and incoming opponent frames through the real
SDK ReplayManager), concurrent writes and preservation beyond former retention limits. Worker tests use injected fake clients
and never log into TETR.IO. `cargo test --locked --no-default-features --all-targets`
covers the adapter protocol and existing search/rule regressions.

`cargo run --release --no-default-features --example funny_bench -- 8 300`
compares solo survival with fixed 7-bag seeds, five previews and a 4x26 field.
The original B2B-first policy reached the 26-row ceiling after 38-74 placements
on these eight seeds. With the headroom policy, all eight reached the 300-piece
cap, with peak heights of 15-17 and longest B2B chains of 15-40. These are offline
checks without opponent garbage, not a live-match survival guarantee.
