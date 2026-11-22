# Aquarius AMM Integration (#656)

Phase 2 venue integration: Aquarius is an automated market maker on Stellar.
Yield comes from constant-product pool fees on USDC pairs.

## Venue summary

| Field        | Value                                                     |
| ------------ | --------------------------------------------------------- |
| Protocol id  | `aquarius`                                                |
| Adapter      | `AquariusAmmAdapter` (`agent/src/protocolAdapters.ts`)     |
| Venue type   | Constant-product AMM                                      |
| Yield source | Swap fees accrued to liquidity positions                  |
| Entry points | `supply`, `withdraw`, `get_apy`, `get_balance`             |

## Adapter behavior

- `supply(ctx, amount)` invokes the adapter contract's `supply` entrypoint
  with `(vault, asset, amount)`. The on-chain adapter wraps single-sided USDC
  deposits into the two-asset pool (splitting into the paired asset as
  needed) and returns the USDC-equivalent deposited amount.
- `withdraw(ctx, amount)` burns the proportional pool position and returns
  USDC; `amount = 0` exits the full position.
- `get_apy(ctx)` returns the trailing fee APR (annualized) as a decimal
  fraction, net of the protocol fee cut.
- `get_balance(ctx)` returns the USDC-equivalent value of the vault's LP
  position.

Responses use the `{ ok, value | error }` envelope; failures become failed
`AdapterResult`s and feed the circuit breaker and error-rate metrics.

## Risk profile (issue #12 scoring inputs)

- **Smart contract risk**: medium — audited AMM core; pool math is well
  understood but immutable after deployment.
- **Liquidity risk**: medium — pool depth on USDC pairs is thinner than
  Blend; large exits suffer slippage, hence the on-chain `min_out` guard.
- **Oracle risk**: medium — LP valuation derives from pool reserves, which
  can diverge from external prices during volatility.
- **Centralization risk**: low–medium — fee setter and pool factory admin.

## Impermanent loss note

A two-asset LP position is exposed to divergence loss versus holding USDC.
The risk-adjusted yield comparison (Sharpe-like metric) in
`yieldComparison.ts` discounts Aquarius APY accordingly; the vault only
routes funds there when the risk-adjusted improvement over the current
protocol exceeds the 0.5% threshold.

## Testing

`agent/src/protocolAdapters.test.ts` covers the four operations with a mock
gateway (including the full-position `withdraw(0)` path), venue failure
surfacing, and the whitelist/risk eligibility gates. On-chain integration
tests should use a mock adapter contract exposing the same four entrypoints,
mirroring `MockDexPool` in `contracts/vault/src/tests/utils.rs`.

## Operational notes

- Withdrawals during volatile periods may return less than requested;
  `min_out` protects the rebalance legs on-chain.
- Fee APY decays as TVL grows; the scheduled rebalancer re-evaluates on its
  configured interval (see `agent/README.md`, "Rebalancing scheduler").
