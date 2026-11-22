# Protocol Adapter Interface (#656)

Phase 2 multi-protocol yield aggregation. This document describes the adapter
pattern used to integrate additional Stellar DeFi venues beyond Blend and the
DEX liquidity pool, and how to add a new venue.

## Overview

Every venue — existing or future — is reached through a uniform adapter that
exposes exactly four operations:

| Operation     | Semantics                                                       |
| ------------- | --------------------------------------------------------------- |
| `supply`      | Deploy USDC (stroops) into the venue. Rejects non-positive amounts. |
| `withdraw`    | Pull USDC back out. `amount = 0` exits the full position.        |
| `get_apy`     | Current annualized yield as a decimal fraction (`0.085` = 8.5%).  |
| `get_balance` | The vault's currently deployed balance in the venue (stroops).    |

Adapters live in `agent/src/protocolAdapters.ts` and talk to venues through a
`SorobanGateway` (read-only `call` + signed `invoke`), so the RPC binding is
injected and mockable in tests.

## Adapter registry, whitelist, and risk gate

`ProtocolAdapterRegistry` decides whether a venue may receive funds:

1. **Registered** — an adapter for the protocol id exists.
2. **Whitelisted** — the owner has enabled the protocol (`setWhitelisted`).
   This mirrors the owner-managed on-chain allowlist checked by `rebalance`
   (`blend`, `dex`, `none` today; new venues are added to the same allowlist
   when their on-chain adapters ship).
3. **Risk-scored** — the composite score from the risk-scoring engine
   (`riskScoring.ts`, issue #12) must be ≤ the registry ceiling
   (`setMaxRiskScore`, default 80).

`assessEligibility(protocolId, riskScores)` returns the decision plus machine
readable reasons (`adapter_not_registered`, `not_whitelisted`,
`risk_score_above_ceiling`) for logging and alerting.

## Adding a new venue

1. Implement `ProtocolAdapter` (extend `GatewayAdapter` and adjust the venue
   entrypoint names/args if the venue contract differs).
2. Add the protocol id to `ProtocolId`.
3. Register it in the agent bootstrap and let the owner whitelist it.
4. Score it with the risk engine and record the scores in the config.
5. Add mock-transport tests (see `protocolAdapters.test.ts`).
6. Document the integration in `docs/<VENUE>_INTEGRATION.md`.

## Relationship to the on-chain contract

On-chain, venues are wrapped by dedicated client structs in
`neurowealth-vault/contracts/vault/src/lib.rs` (`BlendPoolClient`,
`DexPoolClient`) that implement the same supply/withdraw/balance trio plus
`min_out` slippage guards, and protocol legs are gated by the rebalance
allowlist. The agent-side adapter is the aggregation layer that compares
venues (yield comparison engine) and drives on-chain rebalances. A future
Soroban-native generic adapter contract would let *new* venues be added
on-chain without a vault upgrade; the agent interface here is deliberately
shaped to match it.

## Phase 2 venues

- [Phoenix orderbook DEX](PHOENIX_INTEGRATION.md)
- [Aquarius AMM](AQUARIUS_INTEGRATION.md)
