# Formal Verification of Share Accounting

> **Issue:** #672
> **Status:** Kani proofs land in CI; property tests and fuzz targets remain the large-bound complement.

Share accounting is the vault's core economic mechanism. A bug here can mint unbacked shares or pay a withdrawer more than the vault holds. This document specifies the properties we prove, how the proofs are maintained, and how they relate to the on-chain implementation.

## Specification

The contract uses ERC-4626 rounding:

| Direction | Formula | Rounding |
|-----------|---------|----------|
| Assets → shares (deposit mint) | `floor(assets × total_shares / total_assets)` | Floor (vault keeps dust) |
| Assets → shares (withdraw burn) | `ceil(assets × total_shares / total_assets)` | Ceil (user burns ≥ exact) |
| Shares → assets (redeem) | `floor(shares × total_assets / total_shares)` | Floor (user never receives extra) |

Bootstrap (empty vault): `total_shares == 0 \|\| total_assets == 0` maps assets 1:1 onto shares. A zero input always yields a zero output.

These three functions live in the `share-math` crate
([`neurowealth-vault/share-math`](../neurowealth-vault/share-math)) and are
the **only** implementation used by
`NeuroWealthVault::convert_to_shares_internal`,
`convert_to_shares_internal_ceil`, and `convert_to_assets_internal`.
Proving `share-math` therefore proves the on-chain formulas.

## Properties

| ID | Property | Kani proof | Large-bound complement |
|----|----------|------------|------------------------|
| P1 | `total_shares` equals (and therefore never exceeds) the sum of all user share balances | `proof_total_shares_equals_sum_of_user_shares` | `test_total_shares_invariant_proptest.rs`, fuzz `share_accounting_invariants` |
| P2 | No user share balance and not `total_shares` can go negative | `proof_no_negative_share_balance` | deposit/withdraw sequence fuzz |
| P3 | Exchange rate `total_assets / total_shares` is monotonically non-decreasing when yield accrues | `proof_exchange_rate_non_decreasing_with_yield` | `test_exchange_rate.rs` |
| P4 | Deposit then redeem of the minted shares returns `≤` the deposited assets (rounding tolerance: the floor dust) | `proof_deposit_withdraw_round_trip_within_tolerance` | `test_share_conversion_proptest.rs` (a, f, h) |
| P5 | Rounding always favours the vault: ceil-burn ≥ floor-mint (gap ≤ 1) and floor-redeem never pays extra | `proof_rounding_always_favours_the_vault` | `test_share_conversion_proptest.rs` (b, c, g) |

An additional sanity proof, `proof_zero_input_zero_output`, locks the zero-input clause of the spec.

P1/P2 are proved against a two-user `VaultModel` that applies the same floor/ceil/floor helpers the contract uses. P3–P5 are proved directly on those helpers. Kani inputs are bounded to `1..=64` so CBMC finishes in CI; `proptest` reuses the same helpers up to `10^12`.

## Running the proofs

### Locally

Install Kani once:

```bash
cargo install --locked kani-verifier
cargo kani setup
```

Run every proof in the `share-math` crate:

```bash
./scripts/run-kani-proofs.sh
# equivalent:
cd neurowealth-vault && cargo kani -p share-math
```

Run a single proof:

```bash
cd neurowealth-vault
cargo kani -p share-math --harness proof_rounding_always_favours_the_vault
```

Unit tests (no Kani required) cover a handful of concrete cases:

```bash
cd neurowealth-vault && cargo test -p share-math
```

### CI

The `kani-share-math` job in [`.github/workflows/ci.yml`](../.github/workflows/ci.yml)
runs `cargo kani -p share-math` on every pull request and push using
[`model-checking/kani-github-action`](https://github.com/model-checking/kani-github-action).
A failing proof fails the PR.

## Proof maintenance process

Share-accounting changes must keep the proofs green. Use this checklist
whenever you touch mint/burn/redeem math, `TotalShares`, or `TotalAssets`:

1. **Edit `share-math` first.** Do not re-inline the formulas in `lib.rs`.
   The contract must keep calling `shares_floor` / `shares_ceil` /
   `assets_from_shares`.
2. **Update the spec table above** if the rounding policy changes (it
   almost never should).
3. **Extend or loosen a harness only with a written reason.** Widening
   `MAX` increases CI time; lowering it can miss remainder cases.
4. **Keep the `proptest` suite.** Kani is exhaustive in a small box;
   proptest is random in a large box. Both must pass.
5. **Run `./scripts/run-kani-proofs.sh` and `cargo test -p share-math`
   plus `cargo test -p neurowealth-vault share_conversion` before
   opening the PR.**
6. **If a proof fails after a legitimate spec change**, treat it as a
   production incident until the harness is updated *and* the
   property statement in this document is revised in the same PR.

A proof failure on `main` is a release blocker: do not deploy WASM
built from a revision whose Kani job is red.
