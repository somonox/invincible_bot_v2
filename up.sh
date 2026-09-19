#!/usr/bin/env bash
# Linux SSH host: build, install locked dependencies, and start the bot.
set -Eeuo pipefail
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
export PATH="${HOME}/.bun/bin:${HOME}/.cargo/bin:${PATH}"
RUN_DIR="$ROOT/.run"
PID_FILE="$RUN_DIR/bot.pid"
LOG_FILE="$ROOT/logs/bot.log"
INDEX="$ROOT/tetrio-bot/index.ts"
ACTION=${1:-up}
fail() { printf 'ERROR: %s\n' "$*" >&2; exit 1; }
need() { command -v "$1" >/dev/null 2>&1 || fail "Install $1 first (see docs/HOSTING.md)."; }
case "$ACTION" in
  help|--help|-h) echo 'Usage: ./up.sh [up|build|start|restart|stop|status|logs|run]'; exit 0 ;;
  up|build|start|restart|stop|status|logs|run) ;;
  *) fail "Unknown action: $ACTION. Use ./up.sh help." ;;
esac
[[ $(uname -s) == Linux ]] || fail 'up.sh manages Linux servers; use the README for Windows GUI commands.'
cd "$ROOT/tetrio-bot"

# Match the saved PID to this exact bot command and session before signaling.
# A stale PID must never stop an unrelated process after PID reuse.
running() {
  [[ -f "$PID_FILE" ]] || return 1
  read -r BOT_PID < "$PID_FILE" || return 1
  [[ "$BOT_PID" =~ ^[1-9][0-9]*$ ]] || return 1
  [[ -r "/proc/$BOT_PID/cmdline" ]] || return 1
  local arg found=0
  while IFS= read -r -d '' arg; do [[ "$arg" != "$INDEX" ]] || found=1; done < "/proc/$BOT_PID/cmdline"
  [[ $found == 1 ]] || return 1
  [[ $(ps -o sid= -p "$BOT_PID" | tr -d ' ') == "$BOT_PID" ]]
}
build() {
  need cargo; need bun
  (
    local -a extra=()
    if [[ $(uname -m) == aarch64 ]]; then
      # Build on the destination CPU. Overrides allow portable ARM binaries.
      export RUSTFLAGS="${RUSTFLAGS:-} -C target-cpu=${BOT_TARGET_CPU:-native}"
      export CARGO_PROFILE_RELEASE_LTO="${CARGO_PROFILE_RELEASE_LTO:-thin}"
      export CARGO_PROFILE_RELEASE_CODEGEN_UNITS="${CARGO_PROFILE_RELEASE_CODEGEN_UNITS:-1}"
      # Manual NEON was only ~0.3% faster on N1; retain it as an opt-in.
      [[ ${BOT_ARM_NEON:-0} != 1 ]] || extra+=(--features arm-neon)
    fi
    cargo build --manifest-path "$ROOT/Cargo.toml" --locked --release --no-default-features "${extra[@]}" --bin triangle-adapter
  )
  bun install --frozen-lockfile
}
preflight() {
  need bun
  [[ -d "$ROOT/tetrio-bot/node_modules/@haelp/teto" ]] || fail 'Dependencies missing: run ./up.sh build.'
  bun -e 'import {accessSync,constants} from "node:fs"; for (const k of ["BOT_USERNAME","BOT_PASSWORD"]) { if (!process.env[k]) { console.error("Missing " + k + ": configure tetrio-bot/.env or exported environment variables."); process.exit(1); } } const adapter=process.env.BOT_ADAPTER_PATH || "../target/release/triangle-adapter"; try { accessSync(adapter,constants.X_OK); } catch { console.error("Adapter is missing or not executable. Run ./up.sh build, or fix BOT_ADAPTER_PATH."); process.exit(1); }'
}
stop() {
  if ! running; then echo 'Bot is not running (no matching managed PID).'; return; fi
  local pid=$BOT_PID
  kill -TERM -- "-$pid"
  for ((i=0;i<50;i++)); do
    if ! running; then rm -f -- "$PID_FILE"; echo 'Stopped.'; return; fi
    sleep 0.1
  done
  fail 'Bot has not stopped after 5 seconds; inspect it before retrying.'
}
start() {
  if running; then echo "Already running (PID $BOT_PID)."; return; fi
  preflight; need setsid; need nohup
  mkdir -p -- "$ROOT/logs"
  # Close the control lock in the child. setsid gives the bot and its adapters a
  # private process group that stop can terminate together.
  nohup setsid "$(command -v bun)" "$INDEX" 9>&- </dev/null >>"$LOG_FILE" 2>&1 &
  echo "$!" > "$PID_FILE"
  sleep 1
  running || fail "Bot exited at startup. Check $LOG_FILE (credentials are never printed by this script)."
  echo "Started PID $BOT_PID. Logs: $LOG_FILE"
}
service_managed() {
  command -v systemctl >/dev/null 2>&1 &&
    systemctl --user show -p ExecStart --value invincible-bot.service 2>/dev/null |
      grep -Fq -- "$ROOT/up.sh run"
}
# If this checkout is managed by our user service, reuse that supervisor rather
# than accidentally launching a second client alongside it.
if [[ "$ACTION" != run && "$ACTION" != build ]] && service_managed; then
  case "$ACTION" in
    status) exec systemctl --user status --no-pager invincible-bot.service ;;
    logs) exec journalctl --user -u invincible-bot.service -n 100 -f ;;
  esac
fi
case "$ACTION" in
  run) preflight; exec "$(command -v bun)" "$INDEX" ;;
  logs) [[ -f "$LOG_FILE" ]] || fail 'No log file yet.'; exec tail -n 100 -f "$LOG_FILE" ;;
  status) if running; then echo "Running (PID $BOT_PID)."; else echo 'Not running.'; exit 1; fi; exit 0 ;;
esac
need flock
mkdir -p -- "$RUN_DIR"
exec 9>"$RUN_DIR/control.lock"
flock -n 9 || fail 'Another up.sh operation is in progress.'
if service_managed; then
  case "$ACTION" in
    build) build ;;
    start) preflight; systemctl --user start invincible-bot.service ;;
    stop) systemctl --user stop invincible-bot.service ;;
    up|restart) build; preflight; systemctl --user restart invincible-bot.service ;;
  esac
  if [[ "$ACTION" != build && "$ACTION" != stop ]]; then
    sleep 1
    systemctl --user is-active --quiet invincible-bot.service || fail 'Service did not start; run ./up.sh logs.'
  fi
  exit 0
fi
case "$ACTION" in
  build) build ;;
  start) start ;;
  stop) stop ;;
  up|restart) build; preflight; stop; start ;;
esac
