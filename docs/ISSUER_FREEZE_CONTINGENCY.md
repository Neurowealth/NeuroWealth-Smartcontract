# Issuer-Freeze Contingency Plan (Issue #604)

USDC on Stellar is issued as a Stellar Asset Contract (SAC) wrapping a
classic Stellar asset controlled by a centralized issuer (Circle). The
issuer retains the ability to **freeze** (`AUTH_REVOCABLE`/clawback-style
authorization revocation) any account holding that asset, including the
vault contract's own token balance and the AI agent's operational wallet.
This is a centralization risk inherent to any USDC integration, not a bug
in this vault — but its blast radius must be understood and planned for
before mainnet deployment.

This document is the operational plan referenced by
[`SECURITY.md`](../SECURITY.md); see that file's Risk Analysis and
Owner-Compromise Response Runbook for related incident procedures.

## What "frozen" means here

Stellar's `AUTH_REVOCABLE` (and `AUTH_CLAWBACK_ENABLED`) flags let the
issuer of a classic asset deauthorize a trustline for a specific holder
account. On the Soroban side, an SAC-wrapped asset enforces the same
authorization at the token-contract level: `transfer`/`transfer_from` calls
from or to a deauthorized account fail. Two accounts matter here:

1. **The vault contract's own token balance** (the vault contract address
   holds USDC directly — deposits transfer USDC *into* the contract).
2. **The AI agent's wallet** and any protocol pool addresses the vault
   routes funds through (Blend pool, DEX pool) — these hold USDC in their
   own right and could independently be frozen.

## Scenario analysis: impact on each user flow

| Flow | What happens if the **vault contract's** USDC is frozen | What happens if the **agent wallet** is frozen | What happens if a **downstream pool** (Blend/DEX) is frozen |
|------|------|------|------|
| **`deposit`** | Fails at the token layer: the inbound `transfer` from the depositor to the frozen vault address is rejected by the SAC. Deposits are unavailable. | Unaffected directly — the agent does not touch deposit-path transfers. | Unaffected directly. |
| **`withdraw` / `withdraw_all`** | Fails for the idle-balance portion: the outbound `transfer` from the frozen vault address to the user is rejected. If liquidity must first be pulled from a protocol, that inbound leg would also fail once received, compounding the failure. | Unaffected directly — withdrawals move vault→user, not through the agent. | If assets are currently deployed there, the vault's protocol-withdrawal leg fails, so a withdrawal that needs to pull from that pool falls back to the idle-balance-only path (or fails entirely if idle balance is also insufficient). |
| **`rebalance`** | Fails: moving funds into or out of the vault's own balance requires a transfer touching the frozen address. | Fails: `rebalance` requires agent auth (`agent.require_auth()`); if the agent's *wallet* is frozen this is a token-layer failure on any transfer the agent's rebalance triggers, not an auth failure — the call reverts. | Fails for that specific protocol's leg; a rebalance *away* from the frozen pool may itself fail if it needs to withdraw from the frozen pool first. |
| **`harvest` / `emergency_harvest`** | Fails: harvested yield cannot be transferred back to the vault's frozen balance. | Same as rebalance — the agent-initiated transfer is rejected at the token layer. `emergency_harvest` (owner-gated) has the same token-layer exposure since it moves the same USDC. | Fails if yield is being pulled from the frozen pool. |
| **Share price / accounting reads** (`get_total_assets`, `get_shares`, `preview_*`, `convert_to_*`) | **Unaffected.** These are pure reads over on-chain storage state and never call the token contract. Share price freezes at its last-updated value — it does not update further because no transfer that would change `total_assets` can succeed, but it does not go stale-and-wrong, it goes stale-and-frozen. | Unaffected. | Unaffected (the read reflects the vault's own recorded `TotalAssets`, not a live query to the pool). |
| **`pause` / `emergency_pause` / `unpause`** | **Unaffected.** These functions only flip a boolean in instance storage and require owner auth — they never call the USDC token contract, so the owner can always pause the vault even while its USDC balance is frozen. | Unaffected — owner-gated, not agent-gated. | Unaffected. |
| **Ownership/agent-rotation (`transfer_ownership`, `update_agent`, `schedule_upgrade`, etc.)** | Unaffected — these are pure governance state changes with no token transfer. | Unaffected for the same reason. | Unaffected. |

**Bottom line:** a freeze on the vault's own USDC balance is the most severe
case — it blocks deposits and withdrawals outright. A freeze on the agent
wallet or a downstream pool degrades rebalancing/harvesting but the vault's
core "hold funds, let users exit" property is preserved as long as the
*vault contract's own* balance stays unfrozen and there is sufficient idle
USDC to cover requested withdrawals. **Governance functions (pause, owner
rotation) always work regardless of which address is frozen**, because they
never touch the token contract — this is the property the response plan
below relies on.

## Response options

### 1. Pause immediately (always available)

Because `pause()` never touches the USDC token, it is the one action
guaranteed to work even mid-freeze. On any credible signal of a freeze
(user reports of failed transactions, a Circle/issuer public sanctions or
compliance notice, monitoring alerts — see Detection below):

```bash
# Owner-only; works even if the vault's USDC balance is frozen, since this
# call never transfers tokens.
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet \
  -- pause --owner $OWNER_ADDRESS
```

Pausing stops new deposits/withdrawals/rebalances from being attempted and
failing individually (which would otherwise burn user transaction fees for
guaranteed-failing calls), and gives the team a clean state to investigate
from.

### 2. Confirm scope of the freeze

Before communicating anything, determine which address(es) are actually
frozen — the response differs significantly by scope (see the scenario
table above):

```bash
# Check the vault contract's own USDC balance/authorization via Horizon or
# a Stellar RPC `getAccount`/trustline query against the vault's address,
# and separately against the agent wallet and any configured pool addresses.
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_agent
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_blend_pool
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_dex_pool
stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_usdc_token
```

Then check each address's trustline authorization flags (classic Stellar
`AUTHORIZED`/`AUTHORIZED_TO_MAINTAIN_LIABILITIES` state) via Horizon, or
attempt a minimal read-only balance query against the SAC.

### 3. Communicate to users promptly

Freeze incidents are exactly the kind of centralized-counterparty risk
users cannot observe on-chain themselves. Publish, via the established
channels (Twitter, Discord, in-app banner):

- That the vault is paused and why (issuer-level freeze, not a vault bug or
  exploit — this distinction matters for user trust).
- Which flows are affected (per the scenario table above) — e.g. "deposits
  and withdrawals are blocked; your recorded share balance is unaffected
  and unchanged."
- That funds are not lost — they are held at the frozen address(es) and
  will be recoverable once the freeze is lifted or, in the worst case, via
  the issuer's own KYC/compliance-based unfreeze process (see below).
- Expected next update timing, even if it's just "we will update again in
  24h regardless of new information."

### 4. Pursue unfreezing / migration, depending on scope

- **If only a downstream pool (Blend/DEX) is frozen:** the owner can
  rebalance away from that protocol once its idle-balance leg is available,
  or leave the vault paused until the pool's own operator resolves it —
  this does not require issuer engagement at all.
- **If the agent wallet is frozen:** rotate to a new agent address via the
  existing timelocked `update_agent` → `confirm_agent_update` flow (see
  `AGENT_KEY_COMPROMISE_RUNBOOK.md` for the mechanics) once a fresh,
  unfrozen wallet is provisioned. This does not require the issuer at all
  either, since it's a vault-side address change.
- **If the vault contract's own balance is frozen:** this requires the
  issuer's cooperation. Circle (as USDC issuer) has a published process for
  compliance-related freezes, typically involving:
  1. Contacting Circle's compliance/support channel to determine the freeze
     reason (sanctions list match, regulatory order, suspected illicit
     activity, etc.) and required remediation.
  2. Providing any KYC/entity documentation Circle requires to lift the
     freeze on the vault's address specifically.
  3. If unfreezing is not possible in a reasonable timeframe, evaluate a
     **migration path**: deploying a fresh vault contract, and — only if
     the frozen funds are eventually released — a claim/redemption process
     for affected share-holders proportional to their recorded `Shares` at
     the time of the freeze (the `get_users_with_shares` /
     `get_shares` getters remain readable throughout, since reads never
     touch the token contract, so the exact entitlement of every holder is
     always reconstructable even while frozen).
  4. Until resolved, keep the vault paused; do not attempt to work around
     the freeze via alternate token paths (e.g. swapping to a different
     asset) without a full re-audit — that changes the vault's risk model
     entirely.

## Detection

Monitor for these signals as leading indicators of a freeze before it is
publicly confirmed:

| Indicator | Description | Severity |
|-----------|--------------|----------|
| **Deposit/withdraw transactions failing at the token layer** (not at a vault-level `require`) | Users or monitoring report transactions failing with a token-contract authorization error rather than a `VaultError` | HIGH |
| **`rebalance`/`harvest` failing without hitting `RebalanceCooldownActive` or `UnsupportedProtocol`** | Suggests a transfer-layer rejection rather than a normal vault-side guard | HIGH |
| **Public issuer notices** (Circle status page, sanctions list updates) | Circle publishes freeze/blacklist actions publicly in aggregate; monitor for any mention of addresses in the vault's operational set | MEDIUM |
| **Horizon/RPC account queries showing revoked trustline authorization** on the vault, agent, or pool addresses | Direct confirmation | HIGH |

## Related documents

- [`SECURITY.md`](../SECURITY.md) — general risk analysis and the
  owner-compromise incident runbook this plan complements.
- [`AGENT_KEY_COMPROMISE_RUNBOOK.md`](AGENT_KEY_COMPROMISE_RUNBOOK.md) —
  detailed mechanics of agent rotation, relevant if only the agent wallet
  is frozen.
- [`PARTIAL_WITHDRAWAL_BEHAVIOR.md`](PARTIAL_WITHDRAWAL_BEHAVIOR.md) — how
  the vault behaves when requested liquidity is unavailable, relevant
  background for the withdrawal-flow impact analysis above.
