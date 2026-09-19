# Public bot service policy

The online bot requires SRS-X. It spectates and lists incompatible
settings. Give the bot host and issue `!setup` to apply the required settings.
Joining or transferring host alone never changes room options. The actual round engine is
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
  host permission, and the command sender must be the room host or original inviter.
  Unsupported settings continue to block play until fixed.
- `!leave` removes the worker from the room.
- Only the room host or original inviter can use these control commands.
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

Default retention is 14 days, 1,000 files and 2 GiB total. Oldest managed files
are removed first, on startup, hourly and before saving. Files are written
atomically with private permissions and unique names. Unrelated files and
symlinks are excluded from retention. A single saved replay is limited to 32 MiB,
and pending writes to 128 MiB. Recording stops early at 75% of the per-file limit
in received frame bytes to leave space for metadata. It attempts a partial save;
if the serialized result still exceeds the limit, it is rejected and logged.
This bounds normal recording growth; exceptionally large single incoming events
can still temporarily exceed that threshold. Restart/crash before save can lose
an in-progress match. Native playback in the TETR.IO UI still needs a live-match
check; tests validate the saved JSON and lifecycle, not the game's replay viewer.

| Environment variable | Default | Allowed range |
| --- | --- | --- |
| `BOT_MAX_WORKERS` | 20 | 1-32 |
| `BOT_REPLAY_DAYS` | 14 | 1-365 |
| `BOT_REPLAY_MAX_FILES` | 1000 | 1-10000 |
| `BOT_REPLAY_MAX_MB` | 2048 | 1-16384 MiB |
| `BOT_REPLAY_FILE_MB` | 32 | 1-128 MiB |
| `BOT_LOG_MOVES` | off | `1` enables per-move diagnostics |

Set overrides in `tetrio-bot/.env` and restart to apply them. PPS 5 is a hard
service ceiling, not an environment override. See [hosting commands](HOSTING.md).

## Verification

`cd tetrio-bot && bun node_modules/typescript/bin/tsc --noEmit && bun test`
checks typing, settings/host transitions, speed/command permissions, independent
concurrent searches, process failures, stale rounds, immediate empty-room cleanup, replay
lifecycle, concurrent writes and retention. Worker tests use injected fake clients
and never log into TETR.IO. `cargo test --locked --no-default-features --all-targets`
covers the adapter protocol and existing search/rule regressions.
