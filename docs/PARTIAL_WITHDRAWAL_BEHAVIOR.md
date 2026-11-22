# Withdrawal Fairness Under Liquidity Crunch (Issue #600)

## Scope and correction notice

This document previously described a withdrawal-queue system (a
`withdrawalId`, a `queuedAmount`, automatic FIFO processing on liquidity
recovery, a `nextClaimTime`). **That system does not exist in the contract.**
This revision replaces that description with the vault's actual, current
behavior, read directly from `withdraw()` / `withdraw_all()` in
`neurowealth-vault/contracts/vault/src/lib.rs`, and is consistent with
`SECURITY.md`'s "Withdrawal Guarantees" section.

## Summary

When protocol liquidity is insufficient to fully satisfy a withdrawal, the
vault is **first-come-first-served by transaction order, partial-fill,
no-queue**:

- Whoever's withdrawal transaction lands first on the ledger gets first claim
  on whatever USDC the vault can assemble (idle balance + whatever it can
  pull from the active protocol) at that moment.
- If that amount is less than requested, the caller receives exactly what's
  available, right now, in the same transaction — burning only the
  proportional number of shares for what they actually received.
- The unfulfilled remainder is **not** tracked, queued, or scheduled for
  automatic follow-up anywhere. The user keeps their remaining shares and
  must submit a new `withdraw`/`withdraw_all` call later if they want the
  rest.
- There is no receipt object, no `withdrawalId`, and no per-user "amount
  still owed" ledger entry. The only state that persists between attempts is
  the user's own `Shares` balance, which already reflects exactly what they
  are still owed at current share price.

This is **not pro-rata**: two users requesting withdrawals at the same time
with the same liquidity crunch are not guaranteed to receive the same
fraction of their request. Whichever transaction is included first (by
ledger/transaction ordering, which depends on submission time and
network-level ordering, not vault logic) drains available liquidity first;
a later transaction in the same crunch may receive nothing, a partial amount,
or fail outright with `InsufficientLiquidity` / `NoLiquidityAvailable` if the
vault is left with zero USDC.

## Why this design and not a real queue

- **Simplicity and auditability**: no queue state means no separate accounting
  system that could itself have bugs (stuck queue entries, double-claims,
  claim-ordering exploits). The share balance itself is the single source of
  truth for "how much is this user still owed."
- **No custodial holding period**: the vault never promises a specific future
  payout amount or time it cannot control (Blend/DEX liquidity recovery is
  outside the vault's control). A queue with a fabricated `nextClaimTime`
  would be a promise the contract cannot actually guarantee to keep.
- **User optionality preserved**: because unfulfilled shares are never burned,
  a user who receives a partial fill can choose to wait for their own
  strategy reasons (e.g., yield still accruing on the unwithdrawn portion)
  rather than being auto-drained by a queue-processing job the moment
  liquidity reappears.

## Walkthrough

Pool state: `TotalAssets = 1,000 USDC`, only `150 USDC` idle/available from
the active protocol right now.

1. **Alice** calls `withdraw(200 USDC)` first (her transaction lands first).
   - Vault balance check finds only 150 USDC available after attempting to
     pull from the active protocol.
   - `actual_to_return = 150`. Alice's shares are burned only for the 150 USDC
     she actually receives; her remaining share balance corresponds to the
     other 50 USDC she is still owed.
   - Vault now holds `0` USDC.
2. **Bob** calls `withdraw(100 USDC)` immediately after, before any new
   liquidity arrives.
   - Vault balance check finds `0` USDC available (protocol pull also yields
     nothing if it's still fully utilized).
   - The call reverts with `VaultError::InsufficientLiquidity` (via
     `withdraw`) or `NoLiquidityAvailable` (via `withdraw_all`). Bob's shares
     are untouched — this is a clean revert, not a partial fill of zero.
3. **New liquidity arrives** (e.g., Blend utilization drops, or the agent
   rebalances back to idle).
   - Nothing happens automatically. Alice must call `withdraw` again for her
     remaining 50 USDC; Bob must retry his original 100 USDC request. Whoever
     submits first again wins the race for whatever liquidity is present at
     that moment.

This is intentionally simple: it is transaction-ordering-is-fairness, the
same model as calling `withdraw` on almost any other on-chain lending/vault
protocol without an explicit request-queue feature.

## Known DoS / griefing vectors and mitigation status

| # | Vector | Description | Mitigation status |
|---|---|---|---|
| 1 | **Front-running a large withdrawal** | A user who sees a large pending withdrawal (or a liquidity-thinning rebalance) can race their own smaller withdrawal ahead of it to guarantee a full fill while the larger request gets partially or fully starved. | **Not mitigated, and not mitigable at the vault level.** This is inherent to any first-come-first-served on-chain liquidity model; it is a property of transaction ordering (MEV-adjacent), not a vault bug. Documenting it here is the mitigation: users and integrators should not assume equal treatment during a liquidity crunch. |
| 2 | **Withdrawal-announcement griefing** | The issue's wording asks whether an attacker can grief liquidity planning via "oversized withdrawal announcements." The vault has **no announcement/intent mechanism** — there is no `request_withdrawal()`, no queue entry, nothing published on-chain ahead of the actual `withdraw` call that any off-chain planning system could act on and be fooled by. | **Not applicable / no attack surface exists.** An attacker cannot pre-announce a fake large withdrawal to manipulate agent rebalance decisions, because there is nothing to announce into. (If an off-chain monitoring or agent system is later built to *predict* withdrawal demand from mempool/simulation data, that system would need its own griefing analysis — out of scope here since it doesn't exist in this contract.) |
| 3 | **Repeated small partial-fill draining ("liquidity sniping")** | A user (or several colluding users) submits back-to-back small `withdraw()` calls the instant any liquidity trickles back in, permanently starving a legitimate large withdrawer who cannot get a full fill in one shot. | **Not mitigated by the contract; partially mitigated by economics.** Each `withdraw()` still requires the caller to actually own shares — this isn't free money extraction, just faster-than-others access to your own already-owed funds. It does mean a whale's full exit can be meaningfully delayed by many smaller, faster-submitting holders. See `docs/monitoring.md`'s whale-exit alerting (Issue #599) for the operational side of watching for this. |
| 4 | **Revert-based DoS via `InsufficientLiquidity`** | Could an attacker force `InsufficientLiquidity` reverts for legitimate users by draining the vault to exactly `0` via their own withdrawal, timed to precede a victim's transaction? | **Not an attacker advantage beyond vector #1.** Draining the vault to 0 to cause a victim's revert requires the attacker to actually withdraw funds they are legitimately owed — they gain nothing beyond what vector #1 already describes (front-running their own honest withdrawal). There's no way to cause a revert for others without also withdrawing real value for yourself, which bounds this to the attacker's own share balance. |
| 5 | **Circuit-breaker interaction** | Could repeated liquidity-crunch withdrawal failures be used to trip the agent's consecutive-failure circuit breaker and force an unwanted auto-pause? | **Not applicable.** The circuit breaker (`MaxConsecutiveFailures`, see `ARCHITECTURE.md`'s Agent Update Timelock / circuit-breaker sections) counts **rebalance** outcomes reported by the agent, not user `withdraw`/`withdraw_all` call failures. A wave of reverted user withdrawals during a crunch does not touch `ConsecutiveFailures` at all. |

## What "fairness policy" means here, explicitly

> **Stance**: NeuroWealth Vault withdrawals are **not pro-rata** during a
> liquidity crunch. They are **first-come-first-served by transaction
> inclusion order**, with **no queue, no reservation, and no automatic
> retry**. A partially-filled withdrawal leaves the unfulfilled portion as
> ordinary, still-redeemable shares that the user must withdraw again later.
> This is a deliberate simplicity/auditability trade-off, not an oversight —
> but it does mean withdrawal timing during a crunch has real, uncompensated
> economic consequences for whoever is slower to submit.

Integrators (wallets, dashboards, the AI agent) that surface withdrawal UX to
end users should:
- Never display a "queue position" or "estimated payout time" — none exists.
- Treat any non-full-fill `withdraw()` result as "call again later," not as
  "pending, will auto-complete."
- Surface `InsufficientLiquidity` / `NoLiquidityAvailable` reverts as "try
  again shortly," since they mean the vault had zero USDC available at that
  instant, not that the user's shares are at risk.

## Related documentation

- `SECURITY.md` — "Withdrawal Guarantees" section describes the same
  mechanism from a trust-model angle; this document is the detailed
  fairness/DoS analysis referenced there.
- `docs/monitoring.md` — whale-exit concentration risk (Issue #599) covers
  the operational monitoring angle for large holders exiting during thin
  liquidity, which compounds the fairness dynamics described above.
- `ARCHITECTURE.md` — "Idle vs Deployed Asset Tracking" and rebalance flow
  sections describe how the vault decides what liquidity is available to
  satisfy withdrawals in the first place.
