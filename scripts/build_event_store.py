#!/usr/bin/env python3
"""Build Agent-6 typed analytical storage from a normalized market recording."""
from __future__ import annotations

import argparse
import json
from pathlib import Path
import sys

REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "research"))

from agent6_research.event_store import build_event_store


def main() -> None:
    parser = argparse.ArgumentParser(description="Build DuckDB/Parquet from Agent-6 market JSONL")
    parser.add_argument("--input", type=Path, default=REPO_ROOT / "data" / "market-events.jsonl")
    parser.add_argument("--db", type=Path, default=REPO_ROOT / "data" / "research" / "market.duckdb")
    parser.add_argument("--parquet", type=Path, default=REPO_ROOT / "data" / "research" / "market.parquet")
    parser.add_argument("--overwrite", action="store_true")
    args = parser.parse_args()

    manifest = build_event_store(
        args.input,
        args.db,
        args.parquet,
        overwrite=args.overwrite,
    )
    print(json.dumps(manifest, indent=2, sort_keys=True))


if __name__ == "__main__":
    main()
