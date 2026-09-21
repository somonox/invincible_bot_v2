# SSH Linux hosting

The bot consists of the Rust `triangle-adapter` and the Bun client in `tetrio-bot`.
No GUI/display server is required. `up.sh` builds with `--no-default-features`
and installs the dependency versions already recorded in `bun.lock`.
The online adapter uses the same garbage-aware PC/combo policy as the GUI right
bot, with live incoming-packet timing supplied by the Bun client. See
[the policy and prediction limits](HYBRID_POLICY.md).

The service defaults to 20 room workers with a hard PPS cap of 5 and local
`.ttrm` replay storage without automatic expiration or size/count limits. See [required settings, commands and capacity](SERVICE_POLICY.md)
and `tetrio-bot/.env.example` for overrides.

The master prepares one authenticated, roomless spare connection for the next
invitation, then replenishes it after assignment. Workers use the master's token
in memory instead of repeating password authentication; each room still owns an
independent connection. A cold start or simultaneous invitations can still wait
for connection setup. Logs report spare readiness, worker connection time and
room join time separately. The spare does not change the room/search capacity.

## First run

Install a current stable Rust toolchain, Bun, git, a C compiler/linker and Linux
`util-linux` (provides flock/setsid). On Ubuntu, git/build-essential/curl/unzip/
ca-certificates/util-linux are sufficient system prerequisites. See the official
[Rust installer](https://www.rust-lang.org/tools/install) and
[Bun installer](https://bun.com/docs/installation).

```bash
git clone --branch invincible_bot_v2 https://github.com/somonox/invincible_bot_v2.git ~/invincible_bot_v2
cd ~/invincible_bot_v2
# Only on first setup; preserve any existing .env.
cp -n tetrio-bot/.env.example tetrio-bot/.env
chmod 600 tetrio-bot/.env
nano tetrio-bot/.env
./up.sh
```

Set BOT_USERNAME and BOT_PASSWORD to the bot account in that file or the service
environment. Never commit the .env file or an SSH key. Optional BOT_ADAPTER_PATH
can point to a different executable; the default is target/release/triangle-adapter.
The script finds Rust/Bun under ~/.cargo/bin and ~/.bun/bin as well as PATH.
It never installs toolchains or prints account credentials on its own.

## Commands

| Command | Behavior |
| --- | --- |
| `./up.sh` | Build, install locked dependencies, restart the bot |
| `./up.sh build` | Compile/install without restarting |
| `./up.sh start` | Start the already-built bot; avoid a duplicate managed process |
| `./up.sh restart` | Build/install, then restart |
| `./up.sh stop` | Stop the managed bot and its adapter processes |
| `./up.sh status` | Report status (nonzero when stopped) |
| `./up.sh logs` | Follow the log |
| `./up.sh run` | Foreground execution for systemd; no build |

Before a service is installed, start uses nohup/setsid and logs/bot.log. The PID
is checked against this exact checkout and its session before signaling. A file
lock prevents concurrent build/start/stop operations. Manual processes started
outside up.sh are not killed or adopted automatically. Background mode does not
restart after a crash/reboot; use the service below for persistent hosting.

## Persistent user service

The supplied unit assumes the checkout is ~/invincible_bot_v2. Adjust ExecStart
in the unit if it is elsewhere. Build first, and stop any earlier bot before
starting this service so that the same account does not run twice.

```bash
cd ~/invincible_bot_v2
./up.sh build
./up.sh stop
mkdir -p ~/.config/systemd/user
cp deploy/invincible-bot.service ~/.config/systemd/user/
systemctl --user daemon-reload
systemctl --user enable --now invincible-bot.service
sudo loginctl enable-linger "$USER"
./up.sh status
./up.sh logs
```

Once this checkout's service is installed, up/start/restart/stop/status/logs use
systemd automatically. The service restarts on failure, stops its entire process
group on shutdown, and sends logs to the journal. Linger keeps the user service
manager available after logout and at boot; see [loginctl](https://www.freedesktop.org/software/systemd/man/252/loginctl.html).

For subsequent updates:

```bash
cd ~/invincible_bot_v2
git pull --ff-only origin invincible_bot_v2
./up.sh
```

Startup status confirms that the process remains alive, not that account login
or a match succeeded. Check the log for the login and waiting-for-invites messages.

## Validation

- `cargo test --locked --all-targets`: GUI and engine regressions.
- `cargo test --locked --no-default-features --all-targets`: headless build/tests.
- `python3 tests/up_script_smoke.py` on Linux: fake-tool build, duplicate start,
  restart, stop, missing environment, lock contention and stale-PID protection;
  no TETR.IO login or network is used by that test.

## ARM64 optimized build

On an ARM64 host, `./up.sh build` uses `target-cpu=native`, ThinLTO and one
codegen unit for the Rust adapter. Windows GUI builds keep their existing flags.
The resulting server binary targets the build host; set `BOT_TARGET_CPU=generic`
when an ARM binary must run on older ARM CPUs. Existing Cargo LTO/codegen-unit
environment overrides are respected. `BOT_ARM_NEON=1 ./up.sh build` enables the
optional manual NEON transition counter; it is off by default because the N1
end-to-end difference was small. `./up.sh build` does not start a stopped service.

See [ARM measurements](ARM_OPTIMIZATION.md) for the measured speedup, unchanged
move-sequence checks, limits and reproduction commands.

## Rejoin a room after maintenance

For an explicit recovery, set both `BOT_START_ROOM` and `BOT_START_INVITER` (the
original inviter user ID) in the service environment before startup. The room
uses the normal worker pool, permission checks and `!setup` flow. Clear these
variables after startup for one-time re-entry. Neither variable changes room
settings or grants the bot host permission.
