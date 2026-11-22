# Phoenix Orderbook DEX Integration (#656)

Phase 2 venue integration: Phoenix is an orderbook-based exchange on Stellar.
Yield comes from maker rebates and spread capture rather than pool fees.

## Venue summary

| Field        | Value                                                     |
| ------------ | --------------------------------------------------------- |
| Protocol id  | `phoenix`                                                 |
| Adapter      | `PhoenixDexAdapter` (`agent/src/protocolAdapters.ts`)      |
| Venue type   | Central-limit orderbook (CLOB)                            |
| Yield source | Maker rebates, spread capture on resting bids             |
| Entry points | `supply`, `withdraw`, `get_apy`, `get_balance`             |

## Adapter behavior

- `supply(ctx, amount)` invokes the adapter contract's `supply` entrypoint
  with `(vault, asset, amount)`; the adapter places and manages resting bids
  internally and returns the accepted amount.
- `withdraw(ctx, amount)` cancels resting orders and returns USDC;
  `amount = 0` exits the full position.
- `get_apy(ctx)` returns the trailing annualized maker yield as a decimal
  fraction. It is refreshed on every yield-comparison poll.
- `get_balance(ctx)` returns the USDC-equivalent value of the vault's
  working orders plus settled cash.

All responses use the `{ ok, value | error }` envelope; non-numeric or
`ok: false` responses are surfaced as failed `AdapterResult`s and count
toward the scheduler circuit breaker and error-rate metrics.

## Risk profile (issue #12 scoring inputs)

- **Smart contract risk**: medium — audited core, but orderbook matching is
  more complex than an AMM swap.
- **Liquidity risk**: low–medium for USDC pairs on major markets; deep books
  make exits predictable, but size can move through multiple price levels.
- **Oracle risk**: low — pricing is derived from the book itself.
- **Centralization risk**: medium — matcher/operator upgrade authority.

Record the accepted scores in the agent config; the registry blocks the venue
automatically if the composite exceeds the ceiling.

## Testing

`agent/src/protocolAdapters.test.ts` covers the four operations with a mock
gateway, invalid-amount rejection, venue failure surfacing, and the
whitelist/risk eligibility gates. On-chain integration tests should use a
mock adapter contract exposing the same four entrypoints, mirroring
`MockDexPool` in `contracts/vault/src/tests/utils.rs`.

## Operational notes

- Withdrawals that partially cancel resting orders may return less than the
  requested amount; the rebalance `min_out` guard applies on-chain.
- Maker yields are variable; the yield comparison engine treats Phoenix APY
  like any other venue and the 0.5% improvement threshold applies.
