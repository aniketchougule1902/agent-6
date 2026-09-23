#!/usr/bin/env python3
"""Train, calibrate, evaluate, and export the Agent-6 champion ML model.

This script:
1. Fetches historical candles from Bybit V5 REST API (or generates realistic synthetic data if offline).
2. Computes causal technical & microstructure features matching the Rust engine.
3. Computes triple-barrier TP-before-SL meta-labels.
4. Splits data chronologically with an embargo gap to prevent lookahead leakage.
5. Fits regularized model weights and fits a held-out Platt probability calibrator.
6. Evaluates benchmark metrics (AUC, Brier score, ECE) against LightGBM and CatBoost challengers.
7. Selects the utility-maximizing NO_TRADE abstention threshold.
8. Exports the verified model artifact with a cryptographic ModelArtifactManifest.
"""

from __future__ import annotations

import argparse
from datetime import datetime, timezone
from hashlib import sha256
import json
from pathlib import Path
import sys
import time
import urllib.error
import urllib.request

import numpy as np

# Ensure research package is accessible
REPO_ROOT = Path(__file__).resolve().parents[1]
sys.path.insert(0, str(REPO_ROOT / "research"))

from agent6_research.artifacts import ModelArtifactManifest
from agent6_research.calibration import expected_calibration_error, fit_calibrator, select_no_trade_threshold
from agent6_research.catboost_challenger import train_catboost_challenger
from agent6_research.meta_label import fit_lightgbm_meta_label


FEATURE_NAMES = [
    "adx14",
    "rsi14",
    "atr_bps",
    "macd_hist_bps",
    "ema9_21_bps",
    "ema21_50_bps",
    "vwap_dev_bps",
    "relative_volume",
    "momentum_5_bps",
    "directional_alignment",
    "imbalance_proxy",
]


def fetch_bybit_klines(symbol: str, interval: str = "1", limit: int = 1000) -> list[dict]:
    """Fetch linear perpetual klines from Bybit V5 REST API."""
    urls = [
        f"https://api.bybit.com/v5/market/kline?category=linear&symbol={symbol}&interval={interval}&limit={min(limit, 1000)}",
        f"https://api.bytick.com/v5/market/kline?category=linear&symbol={symbol}&interval={interval}&limit={min(limit, 1000)}",
    ]
    for url in urls:
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "Agent6-Trainer/1.0"})
            with urllib.request.urlopen(req, timeout=10) as resp:
                data = json.loads(resp.read().decode("utf-8"))
                if data.get("retCode") == 0 and "result" in data and "list" in data["result"]:
                    raw_list = data["result"]["list"]
                    # Bybit returns newest first; reverse to chronological
                    candles = []
                    for row in reversed(raw_list):
                        candles.append({
                            "start_ms": int(row[0]),
                            "open": float(row[1]),
                            "high": float(row[2]),
                            "low": float(row[3]),
                            "close": float(row[4]),
                            "volume": float(row[5]),
                            "turnover": float(row[6]),
                        })
                    if len(candles) >= 120:
                        return candles
        except Exception:
            continue
    return []


def ema_series(values: np.ndarray, period: int) -> np.ndarray:
    out = np.zeros_like(values)
    if len(values) < period:
        return out
    alpha = 2.0 / (period + 1.0)
    out[:period] = np.mean(values[:period])
    for i in range(period, len(values)):
        out[i] = alpha * values[i] + (1.0 - alpha) * out[i - 1]
    return out


def rma_series(values: np.ndarray, period: int) -> np.ndarray:
    out = np.zeros_like(values)
    if len(values) < period:
        return out
    out[:period] = np.mean(values[:period])
    for i in range(period, len(values)):
        out[i] = (out[i - 1] * (period - 1) + values[i]) / period
    return out


def compute_features_and_labels(
    candles: list[dict], horizon: int = 15, risk_mult: float = 1.8
) -> tuple[np.ndarray, np.ndarray, list[dict]]:
    """Compute features and TP-before-SL labels without lookahead in features."""
    closes = np.array([c["close"] for c in candles], dtype=np.float64)
    highs = np.array([c["high"] for c in candles], dtype=np.float64)
    lows = np.array([c["low"] for c in candles], dtype=np.float64)
    volumes = np.array([c["volume"] for c in candles], dtype=np.float64)
    n = len(candles)

    # 1. EMAs
    ema9 = ema_series(closes, 9)
    ema21 = ema_series(closes, 21)
    ema50 = ema_series(closes, 50)

    # 2. RSI 14
    changes = np.diff(closes, prepend=closes[0])
    gains = np.maximum(changes, 0.0)
    losses = np.maximum(-changes, 0.0)
    avg_gain = rma_series(gains, 14)
    avg_loss = rma_series(losses, 14)
    denom = avg_gain + avg_loss
    rsi14 = np.where(denom > 1e-9, 100.0 * avg_gain / denom, 50.0)

    # 3. ATR 14
    tr = np.zeros(n)
    tr[0] = highs[0] - lows[0]
    for i in range(1, n):
        tr[i] = max(highs[i] - lows[i], abs(highs[i] - closes[i - 1]), abs(lows[i] - closes[i - 1]))
    atr14 = rma_series(tr, 14)

    # 4. ADX 14
    plus_dm = np.zeros(n)
    minus_dm = np.zeros(n)
    for i in range(1, n):
        up = highs[i] - highs[i - 1]
        down = lows[i - 1] - lows[i]
        plus_dm[i] = up if up > down and up > 0.0 else 0.0
        minus_dm[i] = down if down > up and down > 0.0 else 0.0
    atr_smooth = rma_series(tr, 14)
    plus_di = np.where(atr_smooth > 1e-9, 100.0 * rma_series(plus_dm, 14) / atr_smooth, 0.0)
    minus_di = np.where(atr_smooth > 1e-9, 100.0 * rma_series(minus_dm, 14) / atr_smooth, 0.0)
    di_sum = plus_di + minus_di
    dx = np.where(di_sum > 1e-9, 100.0 * np.abs(plus_di - minus_di) / di_sum, 0.0)
    adx14 = rma_series(dx, 14)

    # 5. MACD (12, 26, 9)
    fast_ema = ema_series(closes, 12)
    slow_ema = ema_series(closes, 26)
    macd_line = fast_ema - slow_ema
    signal_line = ema_series(macd_line, 9)
    macd_hist = macd_line - signal_line

    # 6. VWAP 20 & Relative Volume
    vwap20 = np.zeros(n)
    rel_vol = np.ones(n)
    for i in range(20, n):
        win_v = volumes[i - 20:i]
        win_p = (highs[i - 20:i] + lows[i - 20:i] + closes[i - 20:i]) / 3.0
        sum_v = np.sum(win_v)
        vwap20[i] = np.sum(win_p * win_v) / sum_v if sum_v > 1e-9 else closes[i]
        base_v = np.mean(win_v)
        rel_vol[i] = volumes[i] / base_v if base_v > 1e-9 else 1.0

    # 7. Other derived metrics
    atr_bps = (atr14 / closes) * 10_000.0
    macd_hist_bps = (macd_hist / closes) * 10_000.0
    ema9_21_bps = ((ema9 - ema21) / closes) * 10_000.0
    ema21_50_bps = ((ema21 - ema50) / closes) * 10_000.0
    vwap_dev_bps = ((closes - vwap20) / closes) * 10_000.0
    momentum_5_bps = np.zeros(n)
    momentum_5_bps[5:] = ((closes[5:] - closes[:-5]) / closes[:-5]) * 10_000.0
    directional_alignment = np.sign(ema9 - ema21) * np.sign(closes - vwap20)

    # 8. Order flow / imbalance proxy from intra-bar pressure
    bar_range = np.maximum(highs - lows, 1e-9)
    imbalance_proxy = (closes - (highs + lows) / 2.0) / (bar_range / 2.0)

    # Stack features
    warmup = 60
    valid_range = range(warmup, n - horizon)
    features_list = []
    labels_list = []
    meta_info = []

    for i in valid_range:
        close = closes[i]
        atr = atr14[i]
        if atr <= 0.0 or not np.isfinite(close):
            continue

        # Setup direction: Long if EMA9 > EMA21, Short if EMA9 < EMA21
        direction = 1.0 if ema9[i] >= ema21[i] else -1.0
        risk = max(atr * 1.2, close * 0.0008)
        tp = close + direction * risk * risk_mult
        sl = close - direction * risk

        # Evaluate future horizon for TP vs SL touch
        future_highs = highs[i + 1:i + 1 + horizon]
        future_lows = lows[i + 1:i + 1 + horizon]

        tp_hit = False
        sl_hit = False

        for fh, fl in zip(future_highs, future_lows):
            if direction > 0:
                if fh >= tp:
                    tp_hit = True
                    break
                if fl <= sl:
                    sl_hit = True
                    break
            else:
                if fl <= tp:
                    tp_hit = True
                    break
                if fh >= sl:
                    sl_hit = True
                    break

        label = 1 if (tp_hit and not sl_hit) else 0

        feat_row = [
            adx14[i],
            rsi14[i],
            atr_bps[i],
            macd_hist_bps[i],
            ema9_21_bps[i],
            ema21_50_bps[i],
            vwap_dev_bps[i],
            rel_vol[i],
            momentum_5_bps[i],
            directional_alignment[i],
            imbalance_proxy[i],
        ]

        if np.all(np.isfinite(feat_row)):
            features_list.append(feat_row)
            labels_list.append(label)
            meta_info.append({
                "ts_ms": candles[i]["start_ms"],
                "price": close,
                "direction": direction,
                "tp": tp,
                "sl": sl,
            })

    x = np.array(features_list, dtype=np.float64)
    y = np.array(labels_list, dtype=np.int8)
    return x, y, meta_info


def train_champion_model(
    x: np.ndarray,
    y: np.ndarray,
    validation_fraction: float = 0.25,
    embargo: int = 15,
    random_seed: int = 42,
) -> dict:
    """Fit regularized linear model, fit Platt calibrator on held-out split, and benchmark challengers."""
    n = len(x)
    val_size = int(n * validation_fraction)
    val_start = n - val_size
    train_end = val_start - embargo

    if train_end < 50:
        raise ValueError(f"Insufficient training rows: {train_end}")

    x_train, y_train = x[:train_end], y[:train_end]
    x_val, y_val = x[val_start:], y[val_start:]

    # 1. Feature normalization
    means = np.mean(x_train, axis=0)
    stds = np.std(x_train, axis=0)
    stds = np.where(stds < 1e-6, 1.0, stds)

    z_train = (x_train - means) / stds
    z_val = (x_val - means) / stds

    # 2. Logistic Regression (Ridge / L2 regularized)
    from sklearn.linear_model import LogisticRegression
    clf = LogisticRegression(C=0.5, solver="lbfgs", random_state=random_seed, max_iter=200)
    clf.fit(z_train, y_train)

    raw_val_prob = clf.predict_proba(z_val)[:, 1]

    # 3. Fit Platt Probability Calibrator on held-out validation set
    calibrator = fit_calibrator(raw_val_prob, y_val, method="platt")
    calibrated_val_prob = calibrator.transform(raw_val_prob)

    # 4. Metrics
    from sklearn.metrics import brier_score_loss, log_loss, roc_auc_score
    auc = float(roc_auc_score(y_val, calibrated_val_prob)) if len(np.unique(y_val)) > 1 else 0.5
    brier = float(brier_score_loss(y_val, calibrated_val_prob))
    ll = float(log_loss(y_val, calibrated_val_prob, labels=[0, 1]))
    ece = float(expected_calibration_error(calibrated_val_prob, y_val, bins=10))

    # 5. Abstention policy
    abstention = select_no_trade_threshold(calibrated_val_prob, y_val, min_coverage=0.15)

    # 6. Benchmark Challengers
    challengers = {}
    try:
        lgbm = fit_lightgbm_meta_label(x, y, FEATURE_NAMES, validation_fraction=validation_fraction, embargo_rows=embargo)
        challengers["lightgbm"] = {
            "auc": round(lgbm.metrics.auc, 4),
            "brier": round(lgbm.metrics.brier, 4),
            "log_loss": round(lgbm.metrics.log_loss, 4),
        }
    except Exception as e:
        challengers["lightgbm"] = {"error": str(e)}

    try:
        cb = train_catboost_challenger(x, y, validation_fraction=validation_fraction, embargo_rows=embargo)
        challengers["catboost"] = {
            "auc": round(cb.report.auc, 4),
            "brier": round(cb.report.brier, 4),
            "log_loss": round(cb.report.log_loss, 4),
        }
    except Exception as e:
        challengers["catboost"] = {"error": str(e)}

    # Platt coefficients: P = 1 / (1 + exp(-(A * logit + B)))
    platt_model = calibrator.model
    platt_a = float(platt_model.coef_[0][0])
    platt_b = float(platt_model.intercept_[0])

    features_config = []
    for idx, name in enumerate(FEATURE_NAMES):
        features_config.append({
            "name": name,
            "mean": float(means[idx]),
            "scale": float(stds[idx]),
            "weight": float(clf.coef_[0][idx]),
        })

    model_payload = {
        "model_version": "champion-v1",
        "intercept": float(clf.intercept_[0]),
        "features": features_config,
        "calibration": {
            "method": "platt",
            "a": platt_a,
            "b": platt_b,
        },
        "abstention": {
            "threshold": float(abstention.threshold),
            "min_coverage": float(abstention.min_coverage),
            "validation_utility": float(abstention.validation_utility),
        },
        "metrics": {
            "auc": round(auc, 4),
            "brier": round(brier, 4),
            "ece": round(ece, 4),
            "log_loss": round(ll, 4),
            "train_samples": int(len(x_train)),
            "validation_samples": int(len(x_val)),
            "validation_win_rate": round(float(np.mean(y_val)), 4),
        },
        "challengers": challengers,
    }

    return model_payload


def export_model_artifact(
    model_payload: dict,
    output_path: Path,
    training_data_id: str,
    code_revision: str = "main",
) -> Path:
    """Serialize artifact and compute canonical manifest with SHA-256 digest."""
    output_path.parent.mkdir(parents=True, exist_ok=True)

    # Compute deterministic SHA256 of the model definition string
    canonical_model_json = json.dumps(model_payload, sort_keys=True, separators=(",", ":"))
    model_sha256 = sha256(canonical_model_json.encode("utf-8")).hexdigest()

    created_at_ms = int(time.time() * 1000)
    manifest = ModelArtifactManifest(
        schema_version=1,
        model_family="platt_linear_meta_label",
        feature_schema_version="a6.features.v1",
        training_data_id=training_data_id,
        code_revision=code_revision,
        created_at_ms=created_at_ms,
        artifact_sha256=model_sha256,
        metrics=model_payload["metrics"],
        params={
            "features_count": len(model_payload["features"]),
            "calibration_method": model_payload["calibration"]["method"],
            "abstention_threshold": model_payload["abstention"]["threshold"],
        },
    )

    artifact_bundle = {
        "manifest": json.loads(manifest.canonical_json()),
        "model_raw": canonical_model_json,
        "model": model_payload,
    }

    with output_path.open("w", encoding="utf-8") as f:
        json.dump(artifact_bundle, f, indent=2)

    return output_path


def main():
    parser = argparse.ArgumentParser(description="Train and set up Agent-6 ML model")
    parser.add_argument("--symbol", default="BTCUSDT", help="Trading symbol (default: BTCUSDT)")
    parser.add_argument("--interval", default="1", help="Kline interval in minutes (default: 1)")
    parser.add_argument("--limit", type=int, default=1000, help="Number of klines to fetch (default: 1000)")
    parser.add_argument(
        "--output",
        type=Path,
        default=REPO_ROOT / "models" / "research_candidate.json",
        help="Output model JSON path",
    )
    args = parser.parse_args()

    print(f"=== Agent-6 ML Model Training & Setup ===")
    print(f"Target symbol: {args.symbol} (interval: {args.interval}m)")

    data_source = "bybit_rest"
    candles = fetch_bybit_klines(args.symbol, interval=args.interval, limit=args.limit)
    if not candles:
        raise RuntimeError("No real market candles available; refusing synthetic fallback")
    print("RESEARCH ONLY: this candle-proxy model cannot pass the live deployment gate.")

    print(f"Loaded {len(candles)} candles. Computing multi-factor features and meta-labels...")
    x, y, meta = compute_features_and_labels(candles)
    print(f"Extracted {len(x)} samples with {x.shape[1]} features.")
    win_rate = np.mean(y) * 100.0
    print(f"Meta-label target distribution: {win_rate:.1f}% positive (TP before SL)")

    print("Fitting model, Platt probability calibrator, and challengers...")
    model_payload = train_champion_model(x, y)

    metrics = model_payload["metrics"]
    print("\n--- Validation Performance ---")
    print(f"AUC:         {metrics['auc']:.4f}")
    print(f"Brier Score: {metrics['brier']:.4f}")
    print(f"ECE:         {metrics['ece']:.4f}")
    print(f"Log Loss:    {metrics['log_loss']:.4f}")
    print(f"Abstention:  Threshold {model_payload['abstention']['threshold']:.3f} (min coverage {model_payload['abstention']['min_coverage']:.1%})")

    if "challengers" in model_payload:
        print("\n--- Challenger Benchmarks ---")
        for name, res in model_payload["challengers"].items():
            print(f"  {name.upper()}: AUC={res.get('auc', 'N/A')}, Brier={res.get('brier', 'N/A')}")

    data_id = f"{data_source}:{args.symbol}:{candles[0]['start_ms']}-{candles[-1]['start_ms']}"
    out_file = export_model_artifact(model_payload, args.output, training_data_id=data_id)
    print(f"\nModel exported successfully to: {out_file}")
    print("=== Model Setup Complete ===")


if __name__ == "__main__":
    main()
