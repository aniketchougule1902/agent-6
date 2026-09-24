from __future__ import annotations

import hashlib
import json
import math
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Any


@dataclass(frozen=True)
class ReplayEvaluationSummary:
    schema_version: str
    source_sha256: str
    completed_round_trips: int
    signal_events: int
    unresolved_signal_events: int
    skipped_no_book: int
    skipped_stale_book: int
    skipped_unfilled: int
    coverage: float
    win_rate_after_costs: float | None
    expectancy_quote: float | None
    profit_factor_after_costs: float | None
    max_drawdown_quote: float
    net_pnl_quote: float
    fees_quote: float
    median_latency_ms: float | None
    p95_latency_ms: float | None

    def to_json(self) -> str:
        return json.dumps(asdict(self), sort_keys=True, indent=2, allow_nan=False) + "\n"


def _finite_number(value: Any, name: str) -> float:
    if isinstance(value, bool) or not isinstance(value, (int, float)):
        raise ValueError(f"{name} must be numeric")
    value = float(value)
    if not math.isfinite(value):
        raise ValueError(f"{name} must be finite")
    return value


def _nonnegative_int(value: Any, name: str) -> int:
    if isinstance(value, bool) or not isinstance(value, int) or value < 0:
        raise ValueError(f"{name} must be a non-negative integer")
    return value


def _percentile(values: list[float], q: float) -> float | None:
    if not values:
        return None
    ordered = sorted(values)
    index = (len(ordered) - 1) * q
    lo = math.floor(index)
    hi = math.ceil(index)
    if lo == hi:
        return ordered[lo]
    weight = index - lo
    return ordered[lo] * (1.0 - weight) + ordered[hi] * weight


def evaluate_replay_report_bytes(raw: bytes) -> ReplayEvaluationSummary:
    source_sha256 = hashlib.sha256(raw).hexdigest()
    try:
        report = json.loads(raw)
    except json.JSONDecodeError as exc:
        raise ValueError("replay report is not valid JSON") from exc
    if not isinstance(report, dict):
        raise ValueError("replay report must be a JSON object")

    trips = report.get("round_trips")
    if not isinstance(trips, list):
        raise ValueError("round_trips must be a list")

    completed = _nonnegative_int(report.get("completed_round_trips"), "completed_round_trips")
    signals = _nonnegative_int(report.get("signal_events"), "signal_events")
    skipped_no_book = _nonnegative_int(report.get("skipped_no_book"), "skipped_no_book")
    skipped_stale = _nonnegative_int(report.get("skipped_stale_book"), "skipped_stale_book")
    skipped_unfilled = _nonnegative_int(report.get("skipped_unfilled"), "skipped_unfilled")
    if completed != len(trips):
        raise ValueError("completed_round_trips does not match round_trips length")

    net_values: list[float] = []
    latency_values: list[float] = []
    seen_ids: set[str] = set()
    previous_close = -1
    for index, trip in enumerate(trips):
        if not isinstance(trip, dict):
            raise ValueError(f"round_trips[{index}] must be an object")
        signal_id = trip.get("signal_id")
        if not isinstance(signal_id, str) or not signal_id:
            raise ValueError(f"round_trips[{index}].signal_id must be non-empty")
        if signal_id in seen_ids:
            raise ValueError(f"duplicate completed signal_id: {signal_id}")
        seen_ids.add(signal_id)
        opened = _nonnegative_int(trip.get("opened_at_ms"), f"round_trips[{index}].opened_at_ms")
        closed = _nonnegative_int(trip.get("closed_at_ms"), f"round_trips[{index}].closed_at_ms")
        if closed < opened:
            raise ValueError("round trip closes before it opens")
        if closed < previous_close:
            raise ValueError("round trips must be ordered by close timestamp")
        previous_close = closed
        net_values.append(_finite_number(trip.get("net_pnl_quote"), f"round_trips[{index}].net_pnl_quote"))
        latency_values.append(float(_nonnegative_int(trip.get("total_latency_ms"), f"round_trips[{index}].total_latency_ms")))

    net_total = _finite_number(report.get("net_pnl_quote"), "net_pnl_quote")
    fees = _finite_number(report.get("fees_quote"), "fees_quote")
    if fees < 0.0:
        raise ValueError("fees_quote cannot be negative")
    if not math.isclose(sum(net_values), net_total, rel_tol=1e-9, abs_tol=1e-9):
        raise ValueError("net_pnl_quote does not equal summed round-trip net PnL")

    wins = sum(value > 0.0 for value in net_values)
    losses = sum(value < 0.0 for value in net_values)
    flats = len(net_values) - wins - losses
    for field, measured in (("wins_after_costs", wins), ("losses_after_costs", losses), ("flat_after_costs", flats)):
        if _nonnegative_int(report.get(field), field) != measured:
            raise ValueError(f"{field} disagrees with round-trip outcomes")

    reported_win_rate = report.get("win_rate_after_costs")
    measured_win_rate = None if not net_values else wins / len(net_values)
    if reported_win_rate is not None:
        reported = _finite_number(reported_win_rate, "win_rate_after_costs")
        if measured_win_rate is None or not math.isclose(reported, measured_win_rate, rel_tol=1e-9, abs_tol=1e-9):
            raise ValueError("win_rate_after_costs disagrees with round-trip outcomes")

    reported_expectancy = report.get("expectancy_quote")
    measured_expectancy = None if not net_values else sum(net_values) / len(net_values)
    if reported_expectancy is not None:
        reported = _finite_number(reported_expectancy, "expectancy_quote")
        if measured_expectancy is None or not math.isclose(reported, measured_expectancy, rel_tol=1e-9, abs_tol=1e-9):
            raise ValueError("expectancy_quote disagrees with round-trip outcomes")

    gains = sum(max(value, 0.0) for value in net_values)
    losses_abs = sum(max(-value, 0.0) for value in net_values)
    profit_factor = gains / losses_abs if losses_abs > 0.0 else None
    equity = peak = 0.0
    max_drawdown = 0.0
    for value in net_values:
        equity += value
        peak = max(peak, equity)
        max_drawdown = max(max_drawdown, peak - equity)

    accounted = completed + skipped_no_book + skipped_stale + skipped_unfilled
    if accounted > signals:
        raise ValueError("completed/skipped signal outcomes exceed emitted signal count")
    unresolved = signals - accounted
    # Coverage is intentionally measured against every emitted signal, not only
    # signals that reached a fill attempt. This prevents a truncated recording or
    # still-open lifecycle from reporting misleading 100% evaluation coverage.
    coverage = completed / signals if signals else 0.0

    return ReplayEvaluationSummary(
        schema_version="agent6-replay-evaluation-v2",
        source_sha256=source_sha256,
        completed_round_trips=completed,
        signal_events=signals,
        unresolved_signal_events=unresolved,
        skipped_no_book=skipped_no_book,
        skipped_stale_book=skipped_stale,
        skipped_unfilled=skipped_unfilled,
        coverage=coverage,
        win_rate_after_costs=measured_win_rate,
        expectancy_quote=measured_expectancy,
        profit_factor_after_costs=profit_factor,
        max_drawdown_quote=max_drawdown,
        net_pnl_quote=net_total,
        fees_quote=fees,
        median_latency_ms=_percentile(latency_values, 0.5),
        p95_latency_ms=_percentile(latency_values, 0.95),
    )


def evaluate_replay_report(path: Path) -> ReplayEvaluationSummary:
    return evaluate_replay_report_bytes(path.read_bytes())
