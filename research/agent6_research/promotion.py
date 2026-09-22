from __future__ import annotations

from dataclasses import dataclass


@dataclass(frozen=True)
class EvaluationReport:
    trades: int
    expectancy_r: float
    profit_factor: float
    max_drawdown_r: float
    brier_score: float
    calibration_error: float
    stressed_expectancy_r: float
    shadow_expectancy_r: float


@dataclass(frozen=True)
class PromotionPolicy:
    min_trades: int = 500
    min_expectancy_r: float = 0.05
    min_profit_factor: float = 1.15
    max_drawdown_r: float = 20.0
    max_calibration_error: float = 0.08
    min_stressed_expectancy_r: float = 0.0
    min_shadow_expectancy_r: float = 0.0

    def accepts(self, candidate: EvaluationReport, champion: EvaluationReport) -> tuple[bool, list[str]]:
        failures: list[str] = []

        if candidate.trades < self.min_trades:
            failures.append(f"insufficient trades: {candidate.trades} < {self.min_trades}")
        if candidate.expectancy_r < self.min_expectancy_r:
            failures.append("candidate expectancy below floor")
        if candidate.profit_factor < self.min_profit_factor:
            failures.append("candidate profit factor below floor")
        if candidate.max_drawdown_r > self.max_drawdown_r:
            failures.append("candidate drawdown above ceiling")
        if candidate.calibration_error > self.max_calibration_error:
            failures.append("candidate calibration error above ceiling")
        if candidate.stressed_expectancy_r < self.min_stressed_expectancy_r:
            failures.append("candidate fails cost/slippage/latency stress")
        if candidate.shadow_expectancy_r < self.min_shadow_expectancy_r:
            failures.append("candidate fails shadow evaluation")
        if candidate.expectancy_r <= champion.expectancy_r:
            failures.append("candidate does not improve expectancy")
        if candidate.max_drawdown_r > champion.max_drawdown_r * 1.15:
            failures.append("candidate increases drawdown by more than 15%")

        return (not failures, failures)
