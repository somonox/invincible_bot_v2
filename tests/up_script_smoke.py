"""Linux process-control regression: fake tools, no network or real credentials."""
import fcntl
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import time

source = Path(__file__).resolve().parents[1] / "up.sh"
with tempfile.TemporaryDirectory(prefix="bot up test ") as tmp:
    root = Path(tmp)
    (root / "tetrio-bot").mkdir()
    (root / "tetrio-bot/index.ts").touch()
    shutil.copyfile(source, root / "up.sh")
    fake_home = root / "home"
    bun = fake_home / ".bun/bin/bun"
    cargo = fake_home / ".cargo/bin/cargo"
    for tool in (bun, cargo):
        tool.parent.mkdir(parents=True, exist_ok=True)
    bun.write_text("""#!/usr/bin/python3
import os,sys,time
from pathlib import Path
if sys.argv[1]=='install':
    assert '--frozen-lockfile' in sys.argv
    Path('node_modules/@haelp/teto').mkdir(parents=True,exist_ok=True)
elif sys.argv[1]=='-e':
    sys.exit(0 if os.environ.get('BOT_USERNAME') and os.environ.get('BOT_PASSWORD') else 1)
else:
    while True: time.sleep(1)
""")
    cargo.write_text(r"""#!/usr/bin/python3
import sys
from pathlib import Path
assert '--locked' in sys.argv and '--no-default-features' in sys.argv
assert sys.argv[-1]=='triangle-adapter'
p=Path('../target/release/triangle-adapter');p.parent.mkdir(parents=True,exist_ok=True)
p.write_text('#!/bin/sh\nexit 0\n');p.chmod(0o755)
""")
    bun.chmod(0o755)
    cargo.chmod(0o755)
    env = {**os.environ, "HOME": str(fake_home), "BOT_USERNAME": "test-only", "BOT_PASSWORD": "test-only"}
    def run(action, ok=True, extra=None):
        result = subprocess.run(["bash", str(root / "up.sh"), action], env={**env, **(extra or {})}, capture_output=True, text=True, timeout=20)
        assert (result.returncode == 0) == ok, (action, result.stdout, result.stderr)
        return result
    pid_file = root / ".run/bot.pid"
    try:
        run("help")
        run("invalid", False)
        run("status", False)
        run("up")
        first = pid_file.read_text()
        run("start")
        assert pid_file.read_text() == first, "duplicate client"
        run("status")
        run("restart")
        assert pid_file.read_text() != first
        # Lock contention must not race another start/stop operation.
        with (root / ".run/control.lock").open("w") as lock:
            fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
            run("stop", False)
        run("status")
        run("stop")
        run("status", False)
        run("start", False, {"BOT_PASSWORD": ""})
        unrelated = subprocess.Popen(["sleep", "60"])
        try:
            pid_file.write_text(str(unrelated.pid) + "\n")
            run("stop")
            assert unrelated.poll() is None, "signaled unrelated stale PID"
        finally:
            unrelated.terminate()
            unrelated.wait()
        print("up.sh smoke checks passed: build/start/idempotency/restart/lock/stop/preflight/stale PID")
    finally:
        run("stop")
