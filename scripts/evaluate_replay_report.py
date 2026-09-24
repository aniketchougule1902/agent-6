#!/usr/bin/env python3
"""Validate and summarize an Agent-6 replay-simulator report offline.

This command never connects to an exchange, trains a model, promotes a challenger,
or places an order. It only derives auditable metrics from an already-produced
normalized-session replay report.
"""
from __future__ import annotations

import argparse
import os
from pathlib import Path
import sys
import tempfile

ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(ROOT / "research"))

from agent6_research.replay_evaluation import evaluate_replay_report  # noqa: E402


def atomic_write(path: Path, content: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(prefix=f".{path.name}.", dir=path.parent, text=True)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as handle:
            handle.write(content)
            handle.flush()
            os.fsync(handle.fileno())
        os.replace(temp_name, path)
    except Exception:
        try:
            os.unlink(temp_name)
        except FileNotFoundError:
            pass
        raise


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument("report", type=Path, help="JSON report emitted by A6_REPLAY_REPORT_OUTPUT")
    parser.add_argument("--output", type=Path, help="optional atomically-written evaluation summary")
    args = parser.parse_args()

    summary = evaluate_replay_report(args.report)
    rendered = summary.to_json()
    if args.output:
        atomic_write(args.output, rendered)
    else:
        sys.stdout.write(rendered)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
