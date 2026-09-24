import json

import pytest

from agent6_research.replay_evaluation import evaluate_replay_report_bytes


def fixture() -> dict:
    trips = [
        {"signal_id": "a", "opened_at_ms": 100, "closed_at_ms": 200, "net_pnl_quote": 2.0, "total_latency_ms": 20},
        {"signal_id": "b", "opened_at_ms": 210, "closed_at_ms": 300, "net_pnl_quote": -1.0, "total_latency_ms": 40},
        {"signal_id": "c", "opened_at_ms": 310, "closed_at_ms": 400, "net_pnl_quote": 0.0, "total_latency_ms": 30},
    ]
    return {
        "signal_events": 6,
        "completed_round_trips": 3,
        "skipped_no_book": 1,
        "skipped_stale_book": 1,
        "skipped_unfilled": 1,
        "net_pnl_quote": 1.0,
        "fees_quote": 0.6,
        "wins_after_costs": 1,
        "losses_after_costs": 1,
        "flat_after_costs": 1,
        "win_rate_after_costs": 1 / 3,
        "expectancy_quote": 1 / 3,
        "round_trips": trips,
    }


def raw(report: dict) -> bytes:
    return json.dumps(report, sort_keys=True, separators=(",", ":")).encode()


def test_summary_is_deterministic_cost_aware_and_hashed():
    payload = raw(fixture())
    first = evaluate_replay_report_bytes(payload)
    second = evaluate_replay_report_bytes(payload)
    assert first == second
    assert first.source_sha256 == second.source_sha256
    assert first.coverage == 0.5
    assert first.win_rate_after_costs == pytest.approx(1 / 3)
    assert first.expectancy_quote == pytest.approx(1 / 3)
    assert first.profit_factor_after_costs == pytest.approx(2.0)
    assert first.max_drawdown_quote == pytest.approx(1.0)
    assert first.median_latency_ms == pytest.approx(30.0)
    assert first.p95_latency_ms == pytest.approx(39.0)
    assert json.loads(first.to_json())["schema_version"] == "agent6-replay-evaluation-v1"


@pytest.mark.parametrize("mutation", [
    lambda r: r.update(completed_round_trips=2),
    lambda r: r.update(net_pnl_quote=99.0),
    lambda r: r.update(wins_after_costs=3),
    lambda r: r.update(win_rate_after_costs=0.99),
    lambda r: r.update(expectancy_quote=99.0),
    lambda r: r["round_trips"].append(dict(r["round_trips"][0])),
])
def test_inconsistent_reports_fail_closed(mutation):
    report = fixture()
    mutation(report)
    with pytest.raises(ValueError):
        evaluate_replay_report_bytes(raw(report))


def test_timestamp_and_nonfinite_values_fail_closed():
    # This mutation violates the stronger per-round-trip invariant first:
    # b closes before its own open timestamp. Assert that exact safety check
    # instead of coupling the test to a later cross-trip ordering check.
    report = fixture()
    report["round_trips"][1]["closed_at_ms"] = 150
    with pytest.raises(ValueError, match="closes before it opens"):
        evaluate_replay_report_bytes(raw(report))

    report = fixture()
    report["round_trips"][0]["net_pnl_quote"] = float("nan")
    with pytest.raises(ValueError, match="finite"):
        evaluate_replay_report_bytes(raw(report))


def test_empty_report_has_zero_coverage_without_inventing_metrics():
    report = fixture()
    report.update(
        signal_events=0,
        completed_round_trips=0,
        skipped_no_book=0,
        skipped_stale_book=0,
        skipped_unfilled=0,
        net_pnl_quote=0.0,
        fees_quote=0.0,
        wins_after_costs=0,
        losses_after_costs=0,
        flat_after_costs=0,
        win_rate_after_costs=None,
        expectancy_quote=None,
        round_trips=[],
    )
    summary = evaluate_replay_report_bytes(raw(report))
    assert summary.coverage == 0.0
    assert summary.win_rate_after_costs is None
    assert summary.expectancy_quote is None
    assert summary.profit_factor_after_costs is None
