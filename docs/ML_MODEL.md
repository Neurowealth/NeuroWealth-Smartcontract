# ML Model for APY Prediction (Issue #650)

## Overview

The NeuroWealth agent integrates an off-chain machine learning model that forecasts APY for each supported protocol (Blend, DEX). Predictions are submitted on-chain via `submit_apy_prediction`, making the agent's inputs fully auditable. When a forecast is available, the agent factors it into rebalance decisions — moving capital proactively rather than reactively.

## Model Architecture

| Property | Value |
|----------|-------|
| Primary model | LSTM (Long Short-Term Memory) time-series network |
| Fallback model | Prophet (Facebook) for protocols with sparse data |
| Prediction horizons | 1 h, 6 h, 24 h |
| Output | `predicted_apy_bps` (current), `apy_1h_bps`, `apy_6h_bps`, `apy_24h_bps`, `confidence_bps` |
| Retraining cadence | Daily (full retrain) + weekly (hyperparameter sweep) |

### LSTM architecture

```
Input features (T=48 hourly timesteps):
  - current_apy_bps        : APY in basis points
  - tvl_stroops            : Total value locked in the protocol
  - utilization_rate_bps   : Borrow utilization (Blend only)
  - vault_deposit_velocity : Deposit rate over last 6 h (off-chain proxy for demand)

LSTM layers:
  - Layer 1: 64 units, return_sequences=True
  - Layer 2: 32 units
  - Dense:   16 units, ReLU
  - Output:  4 units (apy_1h, apy_6h, apy_24h, confidence) — linear activation

Loss: Huber loss (robust to outlier APY spikes)
Optimizer: Adam, lr=1e-3, weight decay 1e-5
```

### Prophet fallback

Used when a protocol has fewer than 30 days of APY history or when the LSTM prediction confidence is below 30% (`confidence_bps < 3000`). Prophet captures weekly seasonality in DeFi activity without requiring a GPU.

## Training Data

| Source | Cadence | Fields |
|--------|---------|--------|
| Blend pool contract events | Per ledger | supply_apy, borrow_apy, total_supply, total_borrow |
| DEX pool state | Per ledger | fee_apy_estimate, tvl, volume_24h |
| Stellar network metrics | Per ledger | ledger_close_time, base_fee |
| Off-chain market sentiment | Hourly | Fear & Greed Index, BTC dominance (optional feature) |

Historical data is stored in the agent's Supabase tables (`apy_history`, `protocol_metrics`). The training pipeline queries these tables, normalises features to `[0, 1]`, and trains the model.

## On-Chain Integration

```
Agent inference loop (off-chain):
  1. Query Supabase for the last 48 hourly APY snapshots per protocol.
  2. Run LSTM inference → (apy_1h, apy_6h, apy_24h, confidence).
  3. Call vault.submit_apy_prediction(prediction) on-chain.

Agent rebalance decision:
  1. Call vault.get_apy_prediction(protocol) for each candidate protocol.
  2. If prediction.confidence_bps >= 5000 (50%) and apy_6h > current_apy * 1.05:
       → Initiate vault.rebalance(target_protocol, prediction.predicted_apy_bps, min_out)
  3. Otherwise: stay in current protocol or fall back to current observed APY.
```

The on-chain `ApyPrediction` struct (in `lib.rs`) stores the full forecast so any indexer or UI can display the agent's reasoning alongside the rebalance event.

## Confidence Intervals

`confidence_bps` represents the model's self-assessed accuracy:
- `>= 8000` (80%) — High confidence; prediction used directly.
- `5000–7999` (50–79%) — Moderate; used but with a more conservative `min_out`.
- `< 5000` (< 50%) — Low; agent falls back to current observed APY.

The LSTM outputs confidence as the inverse of the normalised prediction variance across the ensemble of the last 5 training runs.

## Backtesting

Before each production deployment the training pipeline runs a rolling backtest:

```bash
# From agent/ml/
python backtest.py \
  --start-date 2025-01-01 \
  --end-date   2026-01-01 \
  --window     30d \
  --step       1d \
  --metric     mape   # Mean Absolute Percentage Error
```

Acceptance threshold: MAPE ≤ 15% across all protocols and all three horizons. Runs that exceed this threshold do not deploy the updated model.

## Retraining Pipeline

```
Daily job (agent/ml/train.py):
  1. Pull latest 90 days of APY history from Supabase.
  2. Feature engineering (rolling averages, utilization deltas).
  3. Train LSTM for up to 100 epochs with early stopping (patience=10).
  4. Backtest on last 30 days.
  5. If MAPE ≤ 15%: save model weights to agent/ml/models/<date>.pt and
     update agent/ml/models/latest symlink.
  6. If MAPE > 15%: keep previous weights; emit a Slack alert.

Weekly sweep (agent/ml/sweep.py):
  1. Run Optuna hyperparameter search (50 trials).
  2. Promote best config if it beats the current production MAPE by ≥ 5%.
```

## Fallback Behaviour

If the on-chain `ApyPrediction` for the target protocol is missing (e.g., inference pipeline is down) or is stale (submitted more than 4 h ago), the agent falls back to the most recently observed APY from the protocol's contract events. Tipping and withdrawals are never blocked by prediction failures.
