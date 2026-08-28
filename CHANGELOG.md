# Changelog

All notable changes to this repository are documented in this file.
This changelog is tied to the vault contract `Version` storage value. Each released contract upgrade should add a new entry matching the stored version number.

> **Contributors:** Any PR that changes on-chain contract behavior, emitted events, error codes,
> or the `Version` storage value **must** update this file. Add your changes under `[Unreleased]`
> and note the target `Version` value if an upgrade is planned. See the PR checklist in
> `.github/pull_request_template.md`.

## [Unreleased]
<!-- Add entries below. Format: `- Short description (Issue #N).` -->
- **Agent-compromise adversarial suite (Issue #673):** Added `test_agent_compromise_scenarios.rs` covering owner-only calls, victim withdrawals, storage mutation, pause/upgrade/pool retarget, arbitrary `TotalAssets` manipulation, and deposit front-running. Documented the threat model in `SECURITY.md`.
- **Formal verification of share accounting (Issue #672):** Extracted mint/burn/redeem math into the `share-math` crate, added Kani proofs for conservation, non-negative balances, monotonic exchange rate, round-trip value, and vault-favouring rounding, and wired `cargo kani -p share-math` into CI. Spec and maintenance process: `docs/FORMAL_VERIFICATION.md`.
- **WCAG 2.1 AA vault UI (Issue #668):** Keyboard access, skip link, ARIA labels, AA contrast, rem-based type, axe-core in Vitest, and `docs/ACCESSIBILITY.md`.
- **Push notifications (Issue #669):** Web Push + PWA service worker, per-type preferences, batching, email fallback (Resend/SendGrid), and `docs/NOTIFICATIONS.md`.
- Document `harvest()` in README, ARCHITECTURE, SECURITY, and the state machine; add dedicated idle/deployed asset breakdown tests (Issues #499, #498, #501).
<!-- If this PR bumps get_version(), note the new Version value here. -->
- **Cross-contract call surface audit (Issue #566):** Added comprehensive table of all 8 external contract invocations across USDC Token, Blend Pool, and DEX Pool contracts, detailing expected success paths, revert behaviors, partial-fill mechanisms, vault accounting reactions, and mapping edge cases to regression tests in `ARCHITECTURE.md`.
- **Defense-in-depth reentrancy test suite (Issue #567):** Added `test_reentrancy_defense.rs` featuring a mock re-entrant token (`ReentrantMockToken`) to assert that hypothetical transfer callbacks cannot corrupt share accounting or execute double-withdrawals under Checks-Effects-Interactions (CEI) ordering.
- **Threat model & trust boundaries documentation (Issue #563):** Added comprehensive threat model, visual trust-boundary architecture diagram, explicit CAN/CANNOT actor capabilities matrix, external contract call site trust assumption mappings, and alignment verification with `docs/MAINNET_CHECKLIST.md` Sections 1 and 7 in `SECURITY.md`.
- **Stale-state audit & CEI enforcement (Issue #568):** Refactored hot paths (`deposit`, `batch_deposit`, `withdraw`, `withdraw_all`, `rebalance`, and `update_total_assets`) to enforce Checks-Effects-Interactions (CEI). All storage reads and state mutations precede cross-contract calls. Added per-function review notes in `ARCHITECTURE.md`, regression test suite (`test_stale_state_audit.rs`), and a grep-based CI check script (`scripts/check-stale-state-audit.sh`).
- `initialize` now rejects the zero address (the unspendable all-zero ed25519
  account) for `deployer`, `owner`, `agent`, and `usdc_token`, with dedicated
  `VaultError` codes 62-65, so a vault can never be initialized with a
  burned/unusable role address (Issue #434).
- `set_deposit_limits` now rejects a `max` above a new `MAX_DEPOSIT_CEILING`
  (100,000,000,000 raw units), via `VaultError::MaximumDepositExceedsCeiling`
  (code 66), preventing a misconfigured astronomically-high per-transaction
  maximum (Issue #435).
- `set_rebalance_cooldown` now emits `RebalanceCooldownUpdatedEvent`
  (`reb_cd`) with the old and new cooldown interval, so indexers can track
  cooldown changes without polling storage (Issue #436).
- `set_approval_ttl` now emits `ApprovalTtlUpdatedEvent` (`ttl_upd`) with the
  old and new TTL, matching the event pattern already used by `set_caps` and
  `set_deposit_limits` (Issue #437).
- No `Version` bump (pre-mainnet).
- **Shares-only balance accounting (Issue #184):** `DataKey::Balance(Address)`
  is confirmed deprecated and unused by core deposit/withdraw/getter logic;
  the discriminant is retained only to preserve the serialized `DataKey`
  layout across upgrades. `get_balance(user)` derives principal purely from
  `Shares(user)` and the current total-assets/total-shares exchange rate.
  `test_balance_shares_invariant.rs` asserts `get_balance(user)` stays within
  rounding tolerance of `convert_to_assets(get_shares(user))` across the
  vault lifecycle, closing the audit concern about drift between principal
  and shares.
- **Timelocked contract upgrade (Issue #316):** the instant `upgrade()` is
  replaced by a two-step, timelocked flow with a cancel path so a compromised
  owner key can no longer swap WASM with no recovery window.
  - New `schedule_upgrade(owner, new_wasm_hash)` records a pending hash and a
    ≈24-hour (`UPGRADE_TIMELOCK_LEDGERS`) expiry ledger; `execute_upgrade(owner)`
    applies it only after the timelock; `cancel_upgrade(owner)` clears a pending
    upgrade. `get_pending_upgrade()` exposes the pending hash and expiry.
  - New storage keys `DataKey::PendingUpgradeHash` / `UpgradeTimelockExpiry`.
  - New events `UpgradeScheduledEvent` (`upg_sched`) and `UpgradeCancelledEvent`
    (`upg_cncl`); `UpgradedEvent` is now emitted by `execute_upgrade`. See EVENTS.md.
  - Error codes 48–50 are generalized from agent-specific to shared timelock
    names (`TimelockAlreadyPending`, `NoTimelockPending`, `TimelockNotExpired`)
    and reused by both the agent (#317) and upgrade timelocks, since the SDK caps
    `#[contracterror]` enums at 50 cases. Numeric codes are unchanged.
  - No `Version` bump (pre-mainnet).
- Add snapshot tests for the DEX event payloads `DexSupplyEvent`,
  `DexWithdrawEvent`, and `DexPoolConfiguredEvent`, mirroring the existing Blend
  event snapshot tests (Issue #340).
- Add ApprovalTtl test coverage for the DEX supply path: default TTL, configured
  TTL, and min/max bound rejection, mirroring the Blend coverage (Issue #341).
- `deploy-devnet.sh` now writes `OWNER_ADDRESS` to `devnet-contracts.env` so
  `verify-deployment.sh` can run without missing-variable errors (Issue #298).
- Document the `TotalDeposits` vs `TotalAssets` design decision in `lib.rs`,
  `ARCHITECTURE.md`, and `test_total_assets_cap.rs`; `TotalDeposits` is
  intentionally not synced on yield — all cap guards use `TotalAssets`
  (Issue #299).
- Add GitHub issue templates as structured YAML forms (Issue #330).
- Migrate weak `!events.is_empty()` test assertions to strict payload checks (Issue #333).
- Dedicated `TvlCapUpdatedEvent` / `UserDepositCapUpdatedEvent` replace ambiguous
  `LimitsUpdatedEvent` for cap-only updates; indexer migration note added to EVENTS.md (Issue #328).
- CHANGELOG.md now tied to contract `Version` with PR template reminder (Issue #335).
- **DEX liquidity pool integration (Issue #228):** the vault can now deploy USDC
  to a Stellar DEX liquidity pool in addition to Blend, implementing the
  on-chain side of the Balanced/Growth strategies.
  - Added owner-configurable `DataKey::DexPool` with `set_dex_pool` / `get_dex_pool`.
  - Added `supply_to_dex` / `withdraw_from_dex` internal helpers mirroring Blend.
  - `rebalance` now accepts the `"dex"` protocol symbol with `min_out` slippage
    protection; `CurrentProtocol` and `ProtocolChangedEvent` reflect DEX deployments.
  - User `withdraw` / `withdraw_all` pull liquidity back from the DEX when needed.
  - New events: `DexSupplyEvent` (`dex_sup`), `DexWithdrawEvent` (`dex_wd`),
    `DexPoolConfiguredEvent` (`dex_cfg`).
  - New errors: `DexPoolNotConfigured` (#46), `OnlyOwnerCanSetDexPool` (#47).
  - New `dex-devnet` test feature flag. No `Version` bump (additive, pre-mainnet).
  - See `docs/DEX_INTEGRATION.md`.

## [1]
- Initial vault implementation with ERC-4626-inspired share accounting.
- `get_version()` returns the contract version from `DataKey::Version`.
- `UpgradedEvent` emits both `old_version` and `new_version` for on-chain auditability.
