"""Run local validation once per hour for the next 24 hours.

This is a verification schedule. It does not write code or claim that a strategy
has reached a particular win rate.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import time
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
LOG = ROOT / "data" / "hourly-check.jsonl"
CHECKS = (
    ("rust", ["cargo", "test", "--workspace", "--quiet"], ROOT),
    ("research", [sys.executable, "-m", "pytest", "-q", "tests"], ROOT / "research"),
    ("ui", ["npm", "run", "build"], ROOT / "apps" / "ui"),
)


def main() -> None:
    LOG.parent.mkdir(parents=True, exist_ok=True)
    start = time.monotonic()
    for hour in range(24):
        for name, command, cwd in CHECKS:
            began = datetime.now(timezone.utc).isoformat()
            try:
                executable = shutil.which(command[0]) or command[0]
                result = subprocess.run([executable, *command[1:]], cwd=cwd, capture_output=True, text=True, timeout=1200)
                status = result.returncode
                output = (result.stdout + "\n" + result.stderr)[-3000:]
            except (OSError, subprocess.TimeoutExpired) as error:
                status = -1
                output = str(error)
            record = {"hour": hour, "check": name, "started_utc": began, "exit_code": status, "output_tail": output}
            with LOG.open("a", encoding="utf-8") as stream:
                stream.write(json.dumps(record) + "\n")
            print(f"H{hour:02d} {name}: {'PASS' if status == 0 else 'FAIL'}", flush=True)
        if hour < 23:
            time.sleep(max(0.0, start + (hour + 1) * 3600 - time.monotonic()))


if __name__ == "__main__":
    main()
