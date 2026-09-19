# Public bot service policy

The online bot requires SRS-X. It spectates and lists incompatible
settings. Give the bot host and issue `!setup` to apply the required settings.
After a successful `!setup`, the bot returns host to the person who gave it host.
If that person has left or no previous owner was observed, it returns host to
the command sender. Joining or transferring host alone never changes room options. The actual round engine is
checked again before starting the Rust adapter, which also rejects other kicks.

Required room settings: 4x20 board, hold/hard drop/180 rotation enabled,
multiplier combos, combo blocking, perfect clears enabled with 10 garbage,
and personal handling enabled (`room_handling=false`). The client uses ARR 0 and
SDF 41. Supported spin settings (`all`, `all-mini+`, `T-spins`) are preserved;
unsupported explicit spin settings are replaced with `all` when the bot is host.
These checks do not promise exact prediction of every optional TETR.IO rule;
existing [rule limitations](TETRIO_RULES.md) still apply.

## Room commands and capacity

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
- `!bot` displays status and help for anyone in the room.
- The pool defaults to 20 rooms (previously 10), at most two per inviter, with
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
lifecycle, concurrent writes and preservation beyond the former retention limits. Worker tests use injected fake clients
and never log into TETR.IO. `cargo test --locked --no-default-features --all-targets`
covers the adapter protocol and existing search/rule regressions.
