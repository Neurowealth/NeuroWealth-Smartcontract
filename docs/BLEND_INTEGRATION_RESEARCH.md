# Blend Protocol Integration Research

## Overview

This document contains research findings for integrating the NeuroWealth Vault with Blend Protocol's Soroban pool contract for on-chain yield generation.

## Production Soroban Interface (Blend v2)

The vault integrates via **request-based** fund management (not legacy `deposit`/`redeem` names):

| Entrypoint                                           | Purpose                                               |
| ---------------------------------------------------- | ----------------------------------------------------- |
| `submit_with_allowance(from, spender, to, requests)` | Supply assets (request type `0`) after USDC `approve` |
| `submit(from, to, requests)`                         | Withdraw assets (request type `1`)                    |
| `balance(asset, user)`                               | Supplied balance for the vault position               |

Request struct (contract-local mirror):

```rust
struct BlendRequest {
    request_type: u32,  // 0 = supply, 1 = withdraw
    address: Address,   // USDC token
    amount: i128,
}
```

Implementation: `BlendPoolClient` in `neurowealth-vault/contracts/vault/src/lib.rs`.

References:

- https://docs.blend.capital/tech-docs/core-contracts/lending-pool/fund-management
- https://github.com/blend-capital/blend-contracts-v2

## Cross-Contract Call Pattern

```rust
env.invoke_contract::<Val>(
    &pool_address,
    &Symbol::new(env, "submit_with_allowance"),
    args,
);
```

Supply flow:

1. Vault `approve`s the Blend pool for the supply amount.
2. Vault calls `submit_with_allowance` with a type-0 request.
3. Blend pulls USDC via `transfer_from` (authorized sub-invocation).

Withdraw flow:

1. Vault calls `submit` with a type-1 request.
2. Blend transfers USDC back to the vault.

## Testing

| Layer                     | Command                                                   |
| ------------------------- | --------------------------------------------------------- |
| Unit / mock pool          | `cargo test -p neurowealth-vault`                         |
| Blend interface (feature) | `cargo test -p neurowealth-vault --features blend-devnet` |

Manual devnet smoke (replace addresses):

```bash
soroban contract invoke --id "$BLEND_POOL" --network testnet -- balance \
  --asset "$USDC" --user "$VAULT"
```

## Protocol Tracking

`DataKey::CurrentProtocol`:

- `"none"`: Funds not deployed (or idle in vault only)
- `"blend"`: Funds deployed to Blend

`ProtocolChangedEvent` (`proto_chg`) is emitted whenever `CurrentProtocol` changes.

## Rebalance API (agent)

```rust
pub fn rebalance(env: Env, protocol: Symbol, expected_apy: i128, min_out: i128);
```

- `min_out`: minimum assets received per supply/withdraw leg; `0` disables slippage checks.
- `RebalanceEvent.status == "noop"`: no funds moved (e.g. already in Blend with zero idle USDC).

## Security Considerations

1. **Reentrancy**: Blend calls follow state updates where applicable (CEI on protocol transitions).
2. **Incomplete exit**: Rebalance aborts if a protocol switch cannot withdraw the full deployed balance.
3. **Slippage**: Optional `min_out` guard on supply/withdraw legs.

## Status

1. ✅ Research Blend interface (this document)
2. ✅ Implement `BlendPoolClient` with production entrypoints
3. ✅ `ProtocolChangedEvent` for indexers
4. ✅ Rebalance `min_out` slippage guard
5. ✅ No-op rebalance semantics (`status: "noop"`)
6. ⏳ Measure gas on testnet
7. ⏳ Security review of cross-contract call patterns

## References

- Blend GitHub: https://github.com/blend-capital
- Blend Documentation: https://docs.blend.capital
- Soroban SDK Documentation: https://soroban.stellar.org/docs

---

# Bad-Debt Analysis: Socialized Loss Impact on Vault Assets

## Overview

Blend Protocol uses **socialized bad debt** to manage underwater positions and protocol solvency. When collateral liquidations or bad debts exceed reserves, the protocol socializes losses across all suppliers. This mechanism is transparent for accounting but creates a critical asymmetry:

**Vault-side consequence:** `get_balance(user)` for a user (shares × exchange rate) can drop overnight without any vault-level event or withdrawal, because the underlying Blend balance decreases due to protocol-level socialization.

This document analyzes impact vectors, detection mechanisms, and response strategies.

## What Is Socialized Bad Debt?

### Mechanism

Blend tracks a **global exchange rate** for supplied assets:

```text
user_balance = user_shares × (total_assets / total_shares)
```

When bad debts accumulate (collateral auctions fail, liquidations insufficient):

1. **Loss Recognition:** Blend's `total_assets` decreases.
2. **Exchange Rate Collapse:** The ratio `(total_assets / total_shares)` falls.
3. **Pro-Rata Loss:** Every supplier's balance shrinks by the same percentage.
4. **No Event:** No vault-side event is emitted; the NeuroWealth vault sees the loss only via `balance()` queries.

### Example

| State                         | Total Supplied (USDC) | Total Shares | Exchange Rate | Your Shares | Your Balance    |
| ----------------------------- | --------------------- | ------------ | ------------- | ----------- | --------------- |
| **Before Loss**               | 10,000,000            | 10,000,000   | 1.0           | 100,000     | 100,000 USDC    |
| **After $500k Socialization** | 9,500,000             | 10,000,000   | 0.95          | 100,000     | **95,000 USDC** |

Your balance dropped $5,000 with **zero vault transaction**.

---

## Impact on NeuroWealth Vault

### Share Price Derivation

NeuroWealth's per-share value depends on Blend's exchange rate:

```rust
// In vault code
vault_share_price = total_assets / total_shares
                  = (idle_balance + blend_balance) / total_vault_shares
                  = (idle + blend_shares × blend_exchange_rate) / total_vault_shares
```

If `blend_exchange_rate` drops due to bad debt, `vault_share_price` drops identically.

### Affected Functions

1. **`get_balance(user)`**: Returns `user_shares × vault_share_price`. A Blend bad-debt event reduces this **silently**.
2. **`get_exchange_rate()`**: Same calculation as above. Returns a lower rate post-loss.
3. **`convert_to_assets(shares)`**: User expecting to withdraw X shares receives fewer USDC.
4. **`preview_withdraw(assets)`**: User needing to withdraw Y USDC must burn more shares.

### Withdrawal Fairness

**Key principle:** All users are treated **equally** under pro-rata socialization.

- User A with 1,000 shares loses 5% → 950 shares worth
- User B with 2,000 shares loses 5% → 1,900 shares worth
- **Both lose 5%**, so no user is unfairly advantaged or disadvantaged by withdrawal order.

**However**, if:

- User A withdraws immediately after bad debt → receives their pro-rata loss
- User B waits → receives the same pro-rata loss (no hidden recovery)

The fairness model **holds** because there is no "recovery" mechanism in Blend; losses are permanent and instantly socialized.

---

## Detection Signals

Since Blend bad-debt events do **not** trigger vault events, monitoring must be **active** (not event-driven).

### Signal 1: Exchange Rate Monitoring

**Setup:**

```bash
# Periodically (e.g., every hour) fetch:
exchange_rate_t0 = vault.get_exchange_rate()
exchange_rate_t1 = vault.get_exchange_rate()

# If rate drops without a vault withdraw:
if exchange_rate_t1 < exchange_rate_t0 and no_withdraw_event_at_t1:
    # Bad debt socialization detected
    loss_percent = (exchange_rate_t0 - exchange_rate_t1) / exchange_rate_t0
    alert("BLEND_BAD_DEBT", loss_percent)
```

**Threshold:** Drops > 0.5% warrant investigation; > 2% are critical.

### Signal 2: Total Assets vs. Balance Sum

**Setup:**

```rust
// In a monitoring entrypoint (read-only, no auth)
pub fn audit_balance_invariant(env: Env) -> bool {
    let idle = Self::get_idle_balance(&env);
    let deployed = Self::get_deployed_assets(&env);
    let total_assets = idle + deployed;

    // Fetch Blend balance directly
    let blend_balance = blend_pool.balance(usdc_token, vault_address);

    // Should match deployed (within rounding tolerance)
    let drift = (blend_balance - deployed).abs();
    return drift <= 10; // Allow 10 units of rounding error
}
```

If `audit_balance_invariant()` returns `false`, Blend balance is lower than vault's recorded `deployed_assets` — a sign of bad debt.

### Signal 3: User Reporting

Users notice their balance has shrunk:

```
User: "I had 1,000 shares worth $10,000. Now it's worth $9,500. Did I get hacked?"
```

Monitoring should flag when:

- No user withdrawal
- No vault rebalance
- Exchange rate dropped

→ **Root cause:** Blend bad debt, not vault issue.

---

## Worked Example: $500k Bad-Debt Event

### Scenario

- **Vault State (Before):**
  - Total Assets: 10,000,000 USDC
  - Total Shares: 10,000,000 (1:1 ratio for simplicity)
  - Idle: 500,000 USDC
  - Deployed to Blend: 9,500,000 USDC (9,500,000 Blend shares at 1:1 rate)

- **Blend State (Before):**
  - Blend total supplied: 100,000,000 USDC (across all suppliers)
  - Blend total shares: 100,000,000 (1:1 rate)
  - Vault's Blend shares: 9,500,000

- **Bad-Debt Event:** Blend realizes $500,000 in bad debt.

### Socialization Process

**Step 1: Blend recognizes loss**

```
Blend total_assets: 100,000,000 - 500,000 = 99,500,000 USDC
Blend total_shares: 100,000,000 (unchanged)
New Blend exchange rate: 99,500,000 / 100,000,000 = 0.995
```

**Step 2: Vault-side impact (passive)**

```
Vault's Blend balance = vault_blend_shares × blend_exchange_rate
                     = 9,500,000 × 0.995
                     = 9,450,500 USDC
```

**Step 3: Vault state after event**

```
Vault Total Assets: 500,000 (idle) + 9,450,500 (Blend) = 9,950,500 USDC
Vault Total Shares: 10,000,000 (unchanged)
Vault exchange rate: 9,950,500 / 10,000,000 = 0.99505
Loss: (10,000,000 - 9,950,500) / 10,000,000 = 0.495% ≈ 0.5%
```

### User Impact

**User A (1,000 shares):**

| Before Bad Debt                   | After Bad Debt                         | Change               |
| --------------------------------- | -------------------------------------- | -------------------- |
| Balance: 1,000 × 1.0 = 1,000 USDC | Balance: 1,000 × 0.99505 = 995.05 USDC | -4.95 USDC (-0.495%) |

**User B (100,000 shares):**

| Before Bad Debt                       | After Bad Debt                           | Change              |
| ------------------------------------- | ---------------------------------------- | ------------------- |
| Balance: 100,000 × 1.0 = 100,000 USDC | Balance: 100,000 × 0.99505 = 99,505 USDC | -495 USDC (-0.495%) |

**Observation:** Both users lose the **same percentage** (0.495%), confirming pro-rata fairness.

### Withdrawal Fairness Check

**Scenario A: User A withdraws immediately after bad debt**

```
shares_to_burn = 1,000 (all shares)
assets_returned = 1,000 × 0.99505 = 995.05 USDC
```

**Scenario B: User A withdraws 1 year later (no additional bad debt)**

```
shares_to_burn = 1,000 (all shares)
assets_returned = 1,000 × 0.99505 = 995.05 USDC (same)
```

**Conclusion:** Withdrawal order does **not** create unfairness. Each user receives their pro-rata share of remaining assets.

---

## Response Options

### Option 1: Silent Acceptance (Recommended for Non-Critical Losses)

For bad-debt events < 0.5%:

1. **Monitor** the exchange rate via periodic RPC queries.
2. **Log** the drawdown to observability system.
3. **No action** — users are fairly affected; forced vault transactions would waste gas.

**Implementation:**

```bash
# Monitoring script (runs hourly)
cron: monitor_blend_exchange_rate
  - Call vault.get_exchange_rate()
  - Compare to last recorded rate
  - If drop > 0.5%: alert("BLEND_LOSS", drop_percent)
  - If drop > 2%: escalate to critical alert
```

### Option 2: Rebalance Exit (For Material Losses > 2%)

If Blend bad debt becomes severe (> 2% loss), consider exiting the Blend position:

```rust
pub fn emergency_rebalance_exit(env: Env, owner: Address) {
    owner.require_auth();
    Self::require_is_owner(&env, &owner);

    // Move all deployed assets back to idle
    // Rebalance to "none" protocol
    let idle = Self::get_idle_balance(&env);
    let deployed = Self::get_deployed_assets(&env);

    // Trigger withdrawal from Blend
    Self::rebalance(&env, symbol_short!("none"), 0, 0);

    // Total assets are now realized at the new (lower) exchange rate
    // Future rebalancing can redeploy once Blend stabilizes
}
```

**When to trigger:**

- Exchange rate drop > 2%
- Blend social loss announcements (community alerts)
- Multiple loss events in short window (systemic stress)

### Option 3: User Communication

If losses exceed 1%, proactively communicate:

```markdown
### Blend Protocol Bad-Debt Event

On [DATE], Blend Protocol socialized $XXX in bad debt across all suppliers.
NeuroWealth vault was affected pro-rata:

- Your balance: [amount] → [amount - loss]
- Loss: [loss_percent]%
- Root cause: Blend collateral liquidations
- Vault action: None (loss is protocol-level)

Your withdrawal rights are unaffected. You can withdraw your remaining balance anytime.
```

---

## Monitoring Hooks

### Hook 1: Periodic Exchange Rate Polling

**Trigger:** `SessionStart` (or external cron job)

```json
{
  "name": "Monitor Blend Exchange Rate Drift",
  "trigger": "SessionStart",
  "action": {
    "type": "command",
    "command": "bash scripts/monitor-blend-rate.sh"
  }
}
```

**Script (`scripts/monitor-blend-rate.sh`):**

```bash
#!/bin/bash

VAULT_CONTRACT_ID="${VAULT_CONTRACT_ID}"
BLEND_POOL="${BLEND_POOL}"
USDC_TOKEN="${USDC_TOKEN}"

# Fetch current vault exchange rate
CURRENT_RATE=$(stellar contract invoke --id "$VAULT_CONTRACT_ID" --network mainnet \
  -- get_exchange_rate)

# Load previous rate from state
PREVIOUS_RATE=$(cat .blend-rate-cache 2>/dev/null || echo "$CURRENT_RATE")

# Calculate delta
RATE_DIFF=$(( CURRENT_RATE - PREVIOUS_RATE ))

if (( RATE_DIFF < -5000 )); then
  # Drop > 0.05% (conservative threshold)
  echo "ALERT: Blend bad-debt loss detected"
  echo "Previous rate: $PREVIOUS_RATE"
  echo "Current rate: $CURRENT_RATE"
  echo "Change: $RATE_DIFF"
  exit 2  # Block further actions if critical
fi

# Update cache
echo "$CURRENT_RATE" > .blend-rate-cache
exit 0
```

### Hook 2: Pre-Rebalance Auditing

**Trigger:** `PreToolUse` (before `rebalance()` calls)

**Purpose:** Reject rebalance if Blend is in a bad-debt spiral.

```json
{
  "name": "Audit Blend Health Before Rebalance",
  "trigger": "PreToolUse",
  "matcher": "rebalance",
  "action": {
    "type": "command",
    "command": "bash scripts/audit-blend-pre-rebalance.sh"
  }
}
```

### Hook 3: Post-Withdrawal Fairness Check

**Trigger:** `PostToolUse` (after `withdraw()` calls)

**Purpose:** Verify no withdrawal skips happened; total-assets accounting is consistent.

```bash
# Pseudo-logic:
# After each withdrawal:
# 1. Record user balance before and after
# 2. Verify loss is pro-rata (all users lost same %)
# 3. Verify total_shares decreased, total_assets decreased proportionally
# 4. If anomaly detected: alert operator
```

---

## References

- **Blend Protocol Docs:** https://docs.blend.capital
- **Soroban Lending Pools:** https://docs.blend.capital/tech-docs/core-contracts/lending-pool
- **Bad Debt & Liquidations:** https://docs.blend.capital/learn/protocol-design/bad-debt-handling
- **Exchange Rate Math:** [ARCHITECTURE.md - Share Accounting](../ARCHITECTURE.md)
