# DEX Liquidity Pool Integration

## Overview

This document describes how the NeuroWealth Vault integrates with a Stellar DEX
liquidity pool for the **Balanced** and **Growth** yield strategies. It covers
the on-chain interface, strategy switching behaviour, integration assumptions,
and operational considerations for external integrators.

It mirrors the patterns established in [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md).

## Strategy Context

The vault supports three yield protocols selectable by the AI agent:

| Protocol symbol | Strategy | Behaviour |
|---|---|---|
| `"none"` | Conservative (idle) | Funds held as USDC inside the vault. No yield deployed. |
| `"blend"` | Conservative / Balanced | USDC supplied to Blend lending pool; yield accrues via bToken exchange rate. |
| `"dex"` | Balanced / Growth | USDC deployed to a DEX liquidity pool via a single-asset adapter. |

The agent calls `rebalance(protocol, expected_apy, min_out)` to switch. Only
one protocol is active at a time; switching exits the current protocol first,
then enters the new one. If either leg fails, the rebalance aborts via
`RebalanceFailedEvent` and the active protocol is unchanged.

## Integration Assumptions

The following assumptions must hold for the DEX integration to function correctly.
Integrators deploying a production adapter should validate each one before going live:

1. **Single-asset adapter contract.** Real Stellar AMMs (Soroswap, Aquarius,
   Comet) are two-asset constant-product pools. Deploying single-sided USDC
   requires a thin adapter that wraps the AMM's `deposit`/`withdraw` and exposes
   the three entrypoints below. The vault is intentionally decoupled from AMM
   specifics and calls only `add_liquidity`, `remove_liquidity`, and `balance`.

2. **USDC is the sole asset the vault touches.** The adapter is responsible for
   all swap legs, zaps, or LP share conversions. The vault measures outcome by
   its own USDC balance delta, not by any value the adapter reports. A
   misreporting adapter cannot inflate vault accounting.

3. **Atomic pool operations.** `add_liquidity` and `remove_liquidity` must
   complete atomically within a single Soroban invocation. Partial fills that
   silently succeed are indistinguishable from full fills and will lead to
   accounting drift; the adapter must either fill in full or revert.

4. **`balance` returns the current USDC-equivalent position.** The vault reads
   `balance(asset, user)` to verify that the deployment succeeded. If the pool
   quotes LP shares rather than USDC, the adapter must convert internally.

5. **`transfer_from` authorisation is handled by the vault.** The vault
   pre-approves the pool for the supply amount with a TTL equal to
   `DataKey::DexApprovalTtl`. The adapter must not modify or re-use that
   approval for any purpose other than the requested supply.

6. **`min_out` is treated as a hard floor.** If the pool cannot realise at least
   `min_out` USDC on a supply or withdraw leg, the vault reverts with
   `VaultError::MinOutNotMet` (#42). Pools that silently under-fill will trigger
   this revert. Pass `min_out = 0` only in tests or when slippage is genuinely
   irrelevant.

7. **The pool address must be registered before first use.** The owner calls
   `set_dex_pool(owner, pool_address)` once. The vault probes `balance` at
   registration time; a non-conforming address reverts at that point, not at
   first rebalance.

8. **Liquidity routing is exit-first.** When switching from Blend to DEX, the
   vault withdraws all Blend funds and holds them idle before deploying to DEX.
   The vault does not support simultaneous multi-protocol deployment.

## Soroban DEX Interface

The vault deploys a single asset (USDC) into a DEX liquidity pool. The pool is
modelled as a **single-asset liquidity adapter** exposing three entrypoints:

| Entrypoint | Purpose |
|------------|---------|
| `add_liquidity(from, asset, amount, min_out)` | Supply USDC liquidity after `approve` |
| `remove_liquidity(to, asset, amount, min_out)` | Withdraw USDC liquidity |
| `balance(asset, user)` | The vault's current liquidity position |

`min_out` is the caller's slippage floor for the leg. The vault forwards it to
the pool **and** independently enforces it on the realized amount (see
[Slippage](#slippage-protection)), so a partial fill is rejected even if the
pool ignores the hint.

Implementation: `DexPoolClient` in
[`neurowealth-vault/contracts/vault/src/lib.rs`](neurowealth-vault/contracts/vault/src/lib.rs).

> **Note on production pools.** Real Stellar AMMs (e.g. Soroswap pairs, Aquarius,
> Comet) are two-asset constant-product pools whose `deposit`/`swap`/`withdraw`
> signatures differ from the single-asset adapter above. Deploying single-sided
> USDC into such a pool requires an adapter contract that performs the swap/zap
> and returns the realized USDC value. The vault is intentionally decoupled from
> that detail: it calls `add_liquidity`/`remove_liquidity`/`balance` on whatever
> `DexPool` address the owner registers, and measures results by its own USDC
> balance delta. The configured address is expected to be either a compatible
> pool or a thin adapter implementing these three entrypoints.

## Cross-Contract Call Pattern

```rust
env.invoke_contract::<Val>(
    &pool_address,
    &Symbol::new(env, "add_liquidity"),
    args,
);
```

Supply flow (mirrors Blend):

1. Vault `approve`s the DEX pool for the supply amount (TTL = `ApprovalTtl`).
2. Vault authorizes `add_liquidity` with a `transfer_from` sub-invocation.
3. Pool pulls USDC via `transfer_from`.
4. Vault computes the realized amount from its own USDC balance delta.

Withdraw flow:

1. Vault calls `remove_liquidity` (amount `0` ⇒ withdraw the full position).
2. Pool transfers USDC back to the vault.
3. Vault computes the realized amount from its balance delta.

## Owner Configuration

```rust
pub fn set_dex_pool(env: Env, owner: Address, pool_address: Address);
pub fn get_dex_pool(env: Env) -> Option<Address>;
```

- Owner-only. The pool interface is validated by probing `balance` before the
  address is stored (an invalid pool reverts at configuration time).
- Stored under `DataKey::DexPool`; emits `DexPoolConfiguredEvent` (`dex_cfg`).

## Rebalance API (agent)

```rust
pub fn rebalance(env: Env, protocol: Symbol, expected_apy: i128, min_out: i128);
```

- Supported `protocol` symbols: `"blend"`, `"dex"`, `"none"`.
- Switching from one protocol to another exits the current one first; an
  incomplete exit emits `RebalanceFailedEvent` and aborts without mutating state.
- `min_out`: minimum assets per supply/withdraw leg; `0` disables the check.
- `RebalanceEvent.status == "noop"`: no funds moved.

## Protocol Tracking

`DataKey::CurrentProtocol`:

- `"none"`: Funds idle in the vault (or not deployed).
- `"blend"`: Funds deployed to Blend.
- `"dex"`: Funds deployed to the DEX pool.

`ProtocolChangedEvent` (`proto_chg`) is emitted whenever `CurrentProtocol`
changes, including transitions into and out of `"dex"`.

## Slippage Protection

On every DEX leg, `min_out` is enforced by `require_min_out`:

- **Supply**: if the realized supplied amount `< min_out`, the call reverts with
  `VaultError::MinOutNotMet` (`#42`).
- **Withdraw**: if the realized withdrawn amount `< min_out`, the call reverts
  with the same error.

`min_out == 0` disables the check.

## Events

| Event | Topic | When |
|-------|-------|------|
| `DexSupplyEvent` | `dex_sup` | USDC supplied to the DEX pool |
| `DexWithdrawEvent` | `dex_wd` | USDC withdrawn from the DEX pool |
| `DexPoolConfiguredEvent` | `dex_cfg` | DEX pool address configured |

See [`EVENTS.md`](../EVENTS.md) for full schemas.

## Testing

| Layer | Command |
|-------|---------|
| Unit / mock pool | `cargo test -p neurowealth-vault` |
| DEX interface (feature) | `cargo test -p neurowealth-vault --features dex-devnet` |

`tests/test_dex_integration.rs` uses an in-env `MockDexPool` (same pattern as
`MockBlendPool`) and covers: supply/withdraw via rebalance, balance reads,
`CurrentProtocol`/`ProtocolChangedEvent` tracking, `min_out` enforcement,
Blend↔DEX switching, and user withdrawals that pull liquidity back from the DEX.

Manual devnet smoke (replace addresses):

```bash
soroban contract invoke --id "$DEX_POOL" --network testnet -- balance \
  --asset "$USDC" --user "$VAULT"
```

## Security Considerations

1. **Slippage**: `min_out` guard on every DEX supply/withdraw leg.
2. **Incomplete exit**: rebalance aborts (via `RebalanceFailedEvent`) if a
   protocol switch cannot withdraw the full deployed balance.
3. **Pool validation**: `set_dex_pool` probes `balance` before storing the
   address, rejecting non-conforming contracts.
4. **Balance-delta accounting**: realized amounts are derived from the vault's
   own USDC balance, not the pool's return value, so a misreporting pool cannot
   inflate accounting.

## MEV and Sandwich Risk

### Sandwich Attack Scenario

An observer watching the mempool can identify a pending `rebalance()` call before
it lands on-chain. The attacker may then:

1. **Front-run**: Submit a transaction that increases the pool price before the
   vault's DEX swap leg executes.
2. **Execute the vault rebalance**: The vault supplies USDC at the inflated price,
   receiving fewer LP shares or less collateral per unit.
3. **Back-run**: Reverse the attacker's initial trade, profiting from the price
   difference while the vault absorbs the slippage.

### Attack Example (with concrete numbers)

Assume a DEX pool has a USDC-BLND pair with a spot rate of 100 USDC : 10 BLND.

- Attacker observes a pending `rebalance()` that will supply 1,000,000 USDC.
- Attacker front-runs with a large buy of BLND (1M USDC), pushing the rate to
  100 USDC : 8 BLND (20% slippage).
- Vault's `rebalance()` executes: supplies 1,000,000 USDC but receives only
  80,000 BLND instead of the expected 100,000 (loss of 20,000 BLND ≈ 200k USDC).
- Attacker back-runs: sells the BLND bought in step 1, pocketing roughly 180k USDC
  profit (after reverting the price and deducting attacker's gas).

### Mitigations

#### 1. Off-Chain `min_out` Computation

The agent backend **must** compute `min_out` off-chain before submitting the
`rebalance()` call to Soroban. The recommended approach:

1. Query the pool's current state (exchange rate, liquidity depth).
2. Simulate the desired supply/withdraw size to estimate the output.
3. Apply a conservative slippage tolerance (e.g., 0.5% - 2%, depending on pool depth).
4. Set `min_out = simulated_output * (1 - tolerance)`.
5. **Include a freshness check**: only proceed if the pool state has not aged more
   than 1-2 blocks; re-query if stale.

#### 2. `min_out` Enforcement in the Vault

The vault enforces `min_out` on **both** supply and withdraw legs:

- If realized amount `< min_out`, the transaction reverts with
  `VaultError::MinOutNotMet` (#42).
- This acts as a **hard floor**: any sandwich that causes slippage exceeding the
  tolerance will reject the entire rebalance.

#### 3. Agent Backend Guidance

**Before calling `rebalance(protocol, expected_apy, min_out)`:**

- **Do not hardcode `min_out = 0`** in production. A zero floor allows unlimited
  slippage and makes sandwich attacks profitable (attacker is guaranteed a fill).
- **Compute `min_out` dynamically** based on recent pool state. If the pool is
  thinly capitalized, increase the slippage tolerance or defer the rebalance.
- **Avoid scheduled rebalances** at fixed times (every hour, at midnight, etc.).
  Predictability enables sandwich attacks. Use randomized intervals or
  event-driven triggers.
- **Monitor large rebalances** on testnet before production. Test with realistic
  slippage figures to identify pools that are too thin.
- **Consider multi-step rebalances** for very large deployments:
  - Instead of deploying 100M USDC in one `rebalance()`, break it into 5x 20M
    over multiple minutes.
  - This reduces the profit surface for a single sandwich attack.

#### 4. Cost-Benefit of the Attack

For a sandwich to be profitable, the attacker's profit must exceed:

- **Slippage cost**: the price movement the attacker induces.
- **Gas fees**: Soroban transaction costs for front-run + back-run (typically
  1-5 stroops per operation, significant for repeated sandwiches).
- **Liquidity risk**: if the pool is deep and the attacker lacks sufficient
  capital, the back-run may not fully recover the front-run's profit.

**Thin pools are high-risk**: A pool with <5M USDC liquidity is vulnerable. A
pool with >50M USDC is typically resistant (a 20% sandwich requires 50M+ in
attacker capital and bears significant slippage risk).

### Limitations

1. **Soroban's single-slot finality** may reduce MEV surface compared to consensus-layered chains:
   - Transactions in the same ledger cannot be reordered.
   - Observing a pending rebalance in the mempool and sandwiching it requires
     network timing coordination across Soroban + Stellar consensus.
   - In practice, Soroban's 5-second ledger close provides a narrow window, but
     not zero opportunity.

2. **No oracle to frontrun on Soroban yet**: if the vault's off-chain agent uses
   a centralized price feed (e.g., Binance API) to compute `min_out`, a
   sophisticated attacker might manipulate that feed. Using Soroban oracle
   contracts (when available) or decentralized price sources reduces this risk.

3. **Backend private keys**: if the agent backend's private key is compromised,
   the attacker can submit arbitrary `min_out` values (including 0). Secure key
   management is critical.

## Status

1. ✅ Research DEX interface (this document)
2. ✅ `DataKey::DexPool` + owner-configurable pool address
3. ✅ `supply_to_dex` / `withdraw_from_dex` internal helpers (mirror Blend)
4. ✅ `rebalance` supports the `dex` protocol with `min_out` slippage protection
5. ✅ `CurrentProtocol` / `ProtocolChangedEvent` reflect DEX deployments
6. ✅ Integration tests with mock DEX pool
7. ✅ `dex-devnet` feature flag for testnet smoke tests
8. ⏳ Measure gas on testnet
9. ⏳ Production AMM adapter (two-asset pool zap) — out of scope for this issue

## Further Reading

- [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md) — Blend protocol integration, mirrors the DEX approach
- [`UPGRADE_MIGRATION.md`](UPGRADE_MIGRATION.md) — how `DataKey::DexPool` and `DexApprovalTtl` survive contract upgrades
- [`../EVENTS.md`](../EVENTS.md) — full `DexSupplyEvent`, `DexWithdrawEvent`, `DexPoolConfiguredEvent` schemas
- [`../SECURITY.md`](../SECURITY.md) — slippage, incomplete-exit, and pool-validation threat analysis

## References

- Soroswap (Soroban AMM): https://docs.soroswap.finance
- Stellar liquidity pools: https://developers.stellar.org/docs/learn/encyclopedia/sdex/liquidity-on-stellar-sdex-liquidity-pools
- Soroban SDK Documentation: https://soroban.stellar.org/docs
- Blend integration: [`BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md)
