# ERC-4626 Conformance Checklist (Issue #602)

This vault is described in [`README.md`](../README.md) and
[`ARCHITECTURE.md`](../ARCHITECTURE.md) as "ERC-4626 inspired" — it follows
the tokenized-vault accounting pattern (shares represent proportional
ownership of pooled assets, share price floats with accrued yield) but is a
native Soroban contract, not a literal port of the Solidity
[ERC-4626](https://eips.ethereum.org/EIPS/eip-4626) interface, and it is not
itself an SEP-41/token contract (shares are tracked internally, not minted
as a separate fungible token).

This document is a function-by-function diff against the ERC-4626 spec
(cross-checked against both the reference implementation and
[Solady's `ERC4626.sol`](https://github.com/Vectorized/solady/blob/main/src/tokens/ERC4626.sol),
since Solady's rounding conventions and hook structure are the de facto
integrator baseline) so auditors and integrators know the exact semantics
without reverse-engineering them from source.

**Legend:** ✅ Conformant · 🟡 Deviant (documented, intentional) · ⬜ N/A

## Core accounting functions

| ERC-4626 function | Vault equivalent | Status | Notes |
|---|---|---|---|
| `asset()` | [`get_usdc_token`](../neurowealth-vault/contracts/vault/src/lib.rs#L5326) | ✅ | Returns the configured USDC token address. Single-asset only — no multi-asset vaults. |
| `totalAssets()` | [`get_total_assets`](../neurowealth-vault/contracts/vault/src/lib.rs#L4743) | ✅ | Sum of idle + deployed assets across all protocols. See [`get_asset_breakdown`](../neurowealth-vault/contracts/vault/src/lib.rs#L5640) for the idle/deployed split. |
| — (no ERC-4626 equivalent) | [`get_total_deposits`](../neurowealth-vault/contracts/vault/src/lib.rs#L4711) | 🟡 | **Deviation, by design.** ERC-4626 has no principal-only counter. This vault adds one (deposits excluding accrued yield) purely for reporting/analytics. It never drives share pricing — see ARCHITECTURE.md §"TotalDeposits vs TotalAssets". Integrators must use `totalAssets`-equivalent (`get_total_assets`) for any pricing logic, never this. |
| `convertToShares(assets)` | [`convert_to_shares`](../neurowealth-vault/contracts/vault/src/lib.rs#L5146) | ✅ | `floor(assets * totalShares / totalAssets)`, bootstrap case `totalShares == 0 \|\| totalAssets == 0` ⇒ `assets` (1:1 seeding). Matches spec: MUST round down, MUST NOT revert unless due to overflow. |
| `convertToAssets(shares)` | [`convert_to_assets`](../neurowealth-vault/contracts/vault/src/lib.rs#L5186) | ✅ | `floor(shares * totalAssets / totalShares)`, `0` when `totalShares == 0`. Matches spec: MUST round down. |

## Preview functions

| ERC-4626 function | Vault equivalent | Status | Notes |
|---|---|---|---|
| `previewDeposit(assets)` | [`preview_deposit_to_shares`](../neurowealth-vault/contracts/vault/src/lib.rs#L5011) | ✅ | Identical to `convert_to_shares` (no deposit fee exists), rounds down. Spec requires `previewDeposit` to round down and to be no more favorable than the actual `deposit` call — satisfied since both call the same internal helper. |
| `previewMint(shares)` | — | 🟡 | **Not implemented.** There is no `mint(shares)` entrypoint (see below), so there is nothing to preview symmetrically. `preview_shares_to_assets` is the closest analog (see next row) but is documented for *valuation*, not *pre-mint costing*. |
| `previewWithdraw(assets)` | [`preview_withdraw`](../neurowealth-vault/contracts/vault/src/lib.rs#L5103) | ✅ | `ceil(assets * totalShares / totalAssets)` — spec requires `previewWithdraw` to round **up** and be no more favorable than the actual `withdraw`. Matches: this is the vault's one asymmetric-rounding preview, and it's documented as such at the call site. **Caveat vs. spec (documented in source):** in a partial-liquidity scenario (protocol pool can't return the full requested amount), the *actual* shares burned by `withdraw` can differ from this preview, which always assumes full liquidity. This is a real, disclosed deviation from strict ERC-4626 (which assumes previews are always exact for the current block) — see `PARTIAL_WITHDRAWAL_BEHAVIOR.md`. |
| `previewRedeem(shares)` | [`preview_shares_to_assets`](../neurowealth-vault/contracts/vault/src/lib.rs#L5052) | ✅ | `floor(shares * totalAssets / totalShares)` — spec requires `previewRedeem` to round down. Matches. Note this function is documented in-repo primarily as a *position-valuation* helper (`get_shares` → value display) rather than a redeem-preview, but the math and rounding direction are identical to what `previewRedeem` requires. |

## Deposit/mint functions

| ERC-4626 function | Vault equivalent | Status | Notes |
|---|---|---|---|
| `deposit(assets, receiver)` | [`deposit`](../neurowealth-vault/contracts/vault/src/lib.rs#L1511) — `deposit(user, amount)` | 🟡 | **Deviation:** no separate `receiver` parameter — `user` is both the token source and the share recipient (`user.require_auth()` is required, so the caller must own the funds and receive the shares in the same identity). There is no deposit-on-behalf-of-another-address flow. Floor-rounds shares minted, matching spec. |
| `mint(shares, receiver)` | — | 🟡 | **Not implemented — deliberate.** ERC-4626's `mint` lets a caller specify an exact share amount and pay however many assets that costs. This vault only exposes asset-denominated entry (`deposit`); there is no shares-denominated entry point. Integrators expecting `mint()` must compute the required asset amount off-chain (e.g. via `preview_shares_to_assets` as an approximation) and call `deposit` instead. |
| `maxDeposit(receiver)` | [`get_max_deposit`](../neurowealth-vault/contracts/vault/src/lib.rs#L3360) combined with [`get_user_deposit_cap`](../neurowealth-vault/contracts/vault/src/lib.rs#L3333) and [`get_tvl_cap`](../neurowealth-vault/contracts/vault/src/lib.rs#L3318) | 🟡 | **Deviation:** ERC-4626's `maxDeposit` returns a single number (the max a specific receiver could deposit right now, `2**256-1` if unlimited). This vault instead exposes three independent caps a caller must combine themselves: a global per-transaction max (`get_max_deposit`), a per-user cumulative cap (`get_user_deposit_cap`), and a vault-wide TVL cap (`get_tvl_cap`). There is no single view function that pre-computes "how much *can* `receiver` deposit right now accounting for their current position and the vault's current TVL" — an integrator has to fetch `get_shares`/`get_balance` for the user plus `get_total_assets` and derive it. **Recommended follow-up (non-blocking):** a convenience `get_max_depositable(user)` view that does this arithmetic server-side, to fully match `maxDeposit`'s ergonomics. |
| `maxMint(receiver)` | — | ⬜ | N/A — no `mint()` exists to bound. |

## Withdraw/redeem functions

| ERC-4626 function | Vault equivalent | Status | Notes |
|---|---|---|---|
| `withdraw(assets, receiver, owner)` | [`withdraw`](../neurowealth-vault/contracts/vault/src/lib.rs#L1800) — `withdraw(user, amount)` | 🟡 | **Deviation:** no separate `receiver`/`owner` — `user` must authorize (`user.require_auth()`) and is both the share-owner being debited and the asset recipient. No withdraw-on-behalf-of / allowance-style delegation exists (there is no ERC-4626-style `allowance` concept for shares at all, since shares are not a separate token). Ceil-rounds shares burned, matching spec's requirement that `withdraw` round in the vault's favor. |
| `redeem(shares, receiver, owner)` | [`withdraw_all`](../neurowealth-vault/contracts/vault/src/lib.rs#L1951) is the closest analog for "redeem everything"; there is **no** shares-denominated partial redeem | 🟡 | **Deviation:** ERC-4626's `redeem` lets the caller specify an exact **share** amount to burn and receive the resulting assets (floor-rounded). This vault has no equivalent partial-redeem-by-shares entrypoint — only `withdraw` (asset-denominated, ceil-rounded) and `withdraw_all` (redeems the caller's entire share balance). A caller who wants to redeem a specific share count today must convert it to an asset amount via `preview_shares_to_assets` first and call `withdraw`, which is not exact due to the floor/ceil rounding asymmetry between that preview and `withdraw`'s own ceil-burn. |
| `maxWithdraw(owner)` | — (derivable from [`get_shares`](../neurowealth-vault/contracts/vault/src/lib.rs#L4805) + [`preview_shares_to_assets`](../neurowealth-vault/contracts/vault/src/lib.rs#L5052)) | 🟡 | **Deviation:** no dedicated view function. Spec's `maxWithdraw` should also account for available liquidity (a vault with illiquid deployed assets should report a lower max than the owner's full position). This vault's partial-withdrawal design (see below) makes this especially relevant — there is currently no on-chain way to query "how much can actually be withdrawn right now given current protocol liquidity" without attempting the withdrawal. **Recommended follow-up:** expose available-liquidity-aware max, ideally derived from `get_idle_balance` + a liquidity probe against the active protocol. |
| `maxRedeem(owner)` | [`get_shares`](../neurowealth-vault/contracts/vault/src/lib.rs#L4805) | 🟡 | The user's full share balance is the naive max (matches spec for the no-liquidity-constraint case), but as with `maxWithdraw`, it does not account for potential partial-liquidity limits — see `PARTIAL_WITHDRAWAL_BEHAVIOR.md`. |

## Partial-liquidity behavior (structural deviation)

**This is the vault's single most significant behavioral deviation from
ERC-4626**, and it is called out explicitly rather than folded into the
table rows above:

> ERC-4626's `withdraw`/`redeem` are specified as atomic: they either
> deliver the full requested amount or revert. This vault instead
> **partially fulfills** a withdrawal when the active protocol (e.g. Blend)
> has insufficient liquidity — the user receives whatever is available and
> **retains their remaining shares** rather than the call reverting.

This is a deliberate design choice (documented in `SECURITY.md` §"Withdrawal
Guarantees" and `PARTIAL_WITHDRAWAL_BEHAVIOR.md`) to avoid forcing users
into a hard revert-and-retry loop during protocol-wide liquidity crunches,
but it means:
- `withdraw(assets, ...)` does not guarantee the caller receives exactly
  `assets` — this is a hard behavioral divergence from the ERC-4626 spec
  text ("MUST support a withdraw flow where the Shares are burned from
  owner directly... and the assets are transferred to receiver").
- `previewWithdraw`'s exactness guarantee (spec: "MUST return as close to
  and no fewer than the exact amount of Shares that would be burned") only
  holds under the full-liquidity assumption, as noted above.

Integrators building on top of this vault (aggregators, other contracts)
**must not assume atomic withdrawal semantics** and must check the actual
transferred amount rather than assuming it equals the requested amount.

## Security-relevant conformance points

| Concern | Status | Notes |
|---|---|---|
| **Inflation / first-depositor attack** (donate assets directly to inflate share price before a victim's first deposit) | ✅ Tested | Bootstrap case (`totalShares == 0` ⇒ 1:1 minting) plus floor-rounding on mint together make the classic ERC-4626 inflation attack unprofitable. Explicitly regression-tested: `test_direct_donation_does_not_inflate_share_price`, `test_donation_then_victim_deposit_is_fair`, `test_victim_can_withdraw_full_deposit_after_donation`, `test_inflation_resistance_across_sizes`, and `test_inflation_attack_profit_always_below_gas_cost_across_timings` in [`tests/test_inflation_attack.rs`](../neurowealth-vault/contracts/vault/src/tests/test_inflation_attack.rs). Note the vault also cannot receive a "direct donation" transfer the way an ERC-20-based vault can be griefed via `token.transfer(vault, amount)`, since `total_assets` is agent-reported/derived rather than a live `balanceOf` read for the deployed portion — see `ARCHITECTURE.md` for the accounting model. |
| **Rounding direction always favors the vault** | ✅ | Floor on mint/value-out, ceil on burn — see the module doc at the top of [`lib.rs`](../neurowealth-vault/contracts/vault/src/lib.rs) (lines 20–32) and `test_rounding_math.rs` / `test_rounding_small_amounts.rs`. Matches the ERC-4626 spec's rounding-direction guidance for vault safety. |
| **Fee-on-transfer / deflationary asset assumption** | 🟡 Documented, not supported | See `SECURITY.md` §"Fee-on-Transfer / Deflationary Asset Risk" (Issue #603) — out of scope for ERC-4626 conformance per se (the spec is fee-agnostic) but relevant to any integrator assuming `deposit`/`withdraw` amounts are exact. |
| **Reentrancy** | ✅ N/A by platform | Soroban's execution model does not have the same reentrancy surface as EVM; cross-contract calls are still reviewed for ordering (state written before external calls — see the module doc's "Write storage state before external calls" convention throughout `lib.rs`). |

## Summary for auditors/integrators

- **Safe to treat as ERC-4626-equivalent for:** share pricing math
  (`convertToShares`/`convertToAssets` and their preview variants other than
  `previewWithdraw`'s liquidity caveat), rounding-direction safety, and
  inflation-attack resistance.
- **Do NOT assume ERC-4626-equivalent for:** atomicity of withdrawals
  (partial fulfillment is possible and by design), the existence of
  `mint`/`redeem`(partial)/`maxDeposit`/`maxWithdraw` as literal
  entrypoints (they don't exist; equivalent data is derivable from the
  getters listed above), or a `receiver`/`owner` delegation model (there is
  none — the caller is always both source and recipient).
- **Non-blocking follow-ups identified above:** a combined
  `get_max_depositable(user)` view and a liquidity-aware `maxWithdraw`
  equivalent would close the remaining ergonomic gap with the spec without
  changing any accounting semantics.
