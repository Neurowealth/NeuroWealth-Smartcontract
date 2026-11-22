# NeuroWealth Vault — External Audit Preparation Package

> **Document status:** Pre-audit draft — complete all `<TO BE FILLED>` placeholders before handing to the audit firm.

---

## 1. Audit Scope

| Field | Value |
| :--- | :--- |
| **Pinned commit** | `2eba4f3321fde31a4af33934f2fa444a20fb202a` |
| **Contract source under review** | `neurowealth-vault/contracts/vault/src/` |
| **Contract version** | 2 (ERC-4626 share accounting) |
| **Soroban SDK version** | 21.0.0 |
| **Rust edition** | 2021 |

### In scope

- All Rust source files under `neurowealth-vault/contracts/vault/src/`
- Fuzz targets under `neurowealth-vault/fuzz/fuzz_targets/` (as supporting material)
- `contract-spec.json` (canonical function ABI at repo root)

### Out of scope

- Next.js frontend (`frontend/`)
- AI agent backend (`agent/`)
- WhatsApp bot handler (`whatsapp/`)
- Off-chain scripts and tooling (`scripts/`)
- Generated TypeScript client (`packages/vault-client/`)
- Database schema (`db/` / `supabase/`)

---

## 2. Artifact Hashes

### Compiled WASM

The optimised WASM binary is produced by the **Build WASM** job in `.github/workflows/ci.yml` and uploaded as the GitHub Actions artifact **`neurowealth-vault-wasm`** (14-day retention).

| Artifact | SHA-256 |
| :--- | :--- |
| `neurowealth_vault.wasm` | `<TO BE FILLED: run CI on pinned commit and record SHA-256 of neurowealth_vault.wasm>` |

To compute the hash yourself after building:

```bash
sha256sum neurowealth_vault.wasm
```

To build locally and reproduce:

```bash
cd neurowealth-vault
stellar contract build
sha256sum target/wasm32-unknown-unknown/release/neurowealth_vault.wasm
```

### Function ABI

`contract-spec.json` at the repository root is the canonical function ABI, generated from the WASM on every CI run. Auditors should verify that the spec matches the source under review.

---

## 3. Architecture Overview

NeuroWealth Vault is an **ERC-4626-inspired yield vault** implemented as a Soroban smart contract on the Stellar blockchain.

### Core design

- **Share-based accounting.** Depositors receive vault shares. The exchange rate is derived from `TotalAssets / TotalShares`, so yield accrued by the AI agent is automatically reflected in each user's redemption value.
- **Three roles.** `Owner` (administrator), `Agent` (AI rebalancer), and `User` (depositor/withdrawer). Full trust-model analysis: [`SECURITY.md`](../SECURITY.md).
- **Two yield protocols.** Blend Protocol (lending) and the Stellar native DEX (liquidity provision). The active protocol is tracked in contract state; only the Agent may trigger transitions via `rebalance()`.
- **Timelocked upgrades and agent rotation.** Both the WASM upgrade path and agent key rotation require a 24-hour timelock (`UPGRADE_TIMELOCK_LEDGERS = AGENT_TIMELOCK_LEDGERS = 17,280 ledgers`).
- **Emergency pause.** The Owner may pause all state-changing operations at any time.
- **Strict CEI pattern.** All state mutations occur before external calls to prevent reentrancy.
- **Checked arithmetic.** All integer operations use Rust's checked/saturating variants; bare arithmetic operators are rejected by CI lint.

### Key reference documents

| Document | Purpose |
| :--- | :--- |
| [`ARCHITECTURE.md`](../ARCHITECTURE.md) | Storage layout, share accounting math, asset flow diagrams |
| [`docs/state-machine.md`](state-machine.md) | Protocol state machine and valid transitions |
| [`SECURITY.md`](../SECURITY.md) | Trust model, threat analysis, owner-compromise runbook |
| [`EVENTS.md`](../EVENTS.md) | Full event catalogue with topic and data schemas |
| [`docs/BLEND_INTEGRATION_RESEARCH.md`](BLEND_INTEGRATION_RESEARCH.md) | Blend supply/withdraw design and cross-contract call patterns |
| [`docs/DEX_INTEGRATION.md`](DEX_INTEGRATION.md) | DEX strategy behaviour and liquidity routing |
| [`docs/PARTIAL_WITHDRAWAL_BEHAVIOR.md`](PARTIAL_WITHDRAWAL_BEHAVIOR.md) | Partial withdrawal behaviour under Blend high-utilization |

---

## 4. Known Issues & Accepted Risks

The following issues have been identified, assessed, and deliberately accepted by the development team. They are disclosed here in full for auditor awareness.

| ID | Title | Severity | Status | Notes |
| :--- | :--- | :---: | :--- | :--- |
| KI-01 | Agent key is a single EOA on mainnet | HIGH | Accepted — mitigation: 24 h timelock on rotation | Multi-sig agent is planned for post-audit |
| KI-02 | Owner key is a single EOA on mainnet | HIGH | Accepted — mitigation: 24 h upgrade + agent timelock, two-step ownership transfer | Multi-sig owner recommended before mainnet |
| KI-03 | Blend/DEX pool addresses are owner-configurable | MEDIUM | Accepted — owner can point vault at a malicious pool | Runbook in `SECURITY.md`; pool changes emit `DexPoolConfiguredEvent` |
| KI-04 | Soroban persistent entry TTL expiry for dormant users | LOW | Accepted — `touch_user_ttl()` is permissionless; off-chain indexers expected to maintain TTLs | No fund loss; user can restore entry |
| KI-05 | `update_total_assets` bounded loss (10% per call) may not cover large protocol losses | MEDIUM | Accepted — agent is trusted; circuit-breaker caps single-call decrease | Monitoring required |
| KI-06 | Partial withdrawals during Blend high-utilization | LOW | Accepted — user retains shares; can retry | Documented in `docs/PARTIAL_WITHDRAWAL_BEHAVIOR.md` |
| KI-07 | `DataKey::Balance` deprecated key retained in enum | INFO | Accepted — no read/write path active; retained for serialization stability | Auditors: this key is inert |

> **Note to auditors:** Additional issues not listed here should be recorded following the existing severity definitions in [Section 10](#10-auditor-coordination).

---

## 5. Security Controls Summary

### Reentrancy

The **Checks-Effects-Interactions (CEI)** pattern is enforced throughout the contract — all state writes complete before any external cross-contract call is made. Reentrancy protection tests: [`neurowealth-vault/contracts/vault/src/tests/test_legacy_inline.rs`](../neurowealth-vault/contracts/vault/src/tests/test_legacy_inline.rs).

### Integer arithmetic

Checked arithmetic is used on every integer operation. Bare arithmetic operators (`+`, `-`, `*`, `/`) are prohibited in contract source and rejected by `cargo clippy` with `clippy::integer_arithmetic`. This is enforced on every CI run.

### Panic-free contract source

No bare `panic!()` calls are permitted in contract source. Enforced by [`scripts/check-no-bare-panic.sh`](../scripts/check-no-bare-panic.sh) on every CI run.

### Access control

Every public function's authentication requirements are verified against `contract-spec.json` on every CI run via `scripts/check-access-control.sh`. A second gate (`scripts/check-pub-fn-auth.sh`, Issue #611) ensures that any new state-changing public function is explicitly classified before it can be merged.

### Emergency pause

The Owner may call `pause()` at any time to halt all state-changing operations. The pause state is checked at the entry point of every mutating function.

### Upgrade and agent rotation safety

Both WASM upgrades and agent key rotations go through a two-step timelocked flow requiring 17,280 ledgers (~24 hours) between proposal and execution. Cancellation is available to the Owner at any time during the timelock window.

### Ownership transfer

Ownership transfer is a two-step flow (`transfer_ownership` + `accept_ownership`) to prevent accidental or malicious ownership loss.

---

## 6. Fuzz / Property-Test Inventory

All fuzz targets live under `neurowealth-vault/fuzz/fuzz_targets/` and are executed in CI.

**CI bounds:**
- PR (triggered when `vault/src/**` changes): `-runs=1000 -max_total_time=120`
- Weekly scheduled run: `-runs=5000 -max_total_time=300`

| Target | File | What it checks | CI trigger |
| :--- | :--- | :--- | :--- |
| `deposit_withdraw_sequence` | `fuzz/fuzz_targets/deposit_withdraw_sequence.rs` | Multi-user deposit/withdraw sequences; no invariant violation | PR + weekly |
| `share_accounting_invariants` | `fuzz/fuzz_targets/share_accounting_invariants.rs` | I-1 through I-6 from invariant register (sum shares, proportionality, total_assets ≥ total_deposits, exchange_rate ≥ 1.0) | PR + weekly |
| `rounding_boundaries` | `fuzz/fuzz_targets/rounding_boundaries.rs` | Floor-mint / ceil-burn rounding rules; vault never loses value | PR + weekly |
| `rebalance_transitions` | `fuzz/fuzz_targets/rebalance_transitions.rs` | Protocol state machine transitions (blend ↔ dex ↔ none); no stuck state | PR + weekly |
| `agent_update_timelock` | `fuzz/fuzz_targets/agent_update_timelock.rs` | Agent rotation timelock; can't confirm before expiry | PR + weekly |
| `admin_timelock_interleaved` | `fuzz/fuzz_targets/admin_timelock_interleaved.rs` | Interleaved upgrade + agent timelocks; no state corruption | PR + weekly |
| `user_strategy` | `fuzz/fuzz_targets/user_strategy.rs` | Strategy preference set/get; no cross-user contamination | PR + weekly |
| `upgrade_timelock` | `fuzz/fuzz_targets/upgrade_timelock.rs` | WASM upgrade timelock; can't execute before expiry | PR + weekly |

---

## 7. Test Coverage Summary

### Overview

| Metric | Value |
| :--- | :--- |
| Test files | 54 |
| Test location | `neurowealth-vault/contracts/vault/src/tests/` |
| All tests passing on pinned commit | ✅ Yes (`2eba4f3321fde31a4af33934f2fa444a20fb202a`) |

### Modules covered

deposit, withdraw, withdraw_all, rebalance, pause/unpause, access control, events, share math, rounding (floor-mint / ceil-burn), overflow, timelocks (agent rotation + WASM upgrade), TTL/rent, Blend integration, DEX integration, budget/resource regression, multi-user scenarios, yield accrual, inflation attack resistance, strategy preference.

### Notable test files

| Category | Files |
| :--- | :--- |
| Property-based (proptest) | `test_share_conversion_proptest.rs`, `test_total_shares_invariant_proptest.rs`, `test_decrease_cap_proptest.rs` |
| Budget / resource regression | `test_budget.rs` — CPU instruction budget: < 5M (normal) / < 15M (worst-case); memory: < 300K / < 600K per operation |
| Blast-radius analysis (off-chain) | `test/OwnerCompromiseBlastRadius.test.ts`, `test/NotOwnerCompromiseBlastRadius.test.ts` |

### Running the tests

```bash
cd neurowealth-vault
cargo test --verbose
```

---

## 8. Self-Audit / Prior Review Results

### Internal security review

An internal blast-radius analysis has been completed. Results are documented in:

- `test/OwnerCompromiseBlastRadius.test.ts` — enumerates all actions available to a compromised owner key and their impact
- `test/NotOwnerCompromiseBlastRadius.test.ts` — enumerates all actions available to non-owner addresses and confirms they cannot exceed their intended permissions

### Third-party audit history

No prior third-party audit has been performed on this codebase. This engagement is the **first external security review**.

### Known test pass status

All 54 test modules pass on the pinned commit `2eba4f3321fde31a4af33934f2fa444a20fb202a`.

---

## 9. How to Reproduce Locally

The following commands reproduce the full test, fuzz, and static-analysis suite from a clean checkout:

```bash
# 1. Clone and pin to the audited commit
git clone <repo>
git checkout 2eba4f3321fde31a4af33934f2fa444a20fb202a

# 2. Add the WASM compilation target
rustup target add wasm32-unknown-unknown

# 3. Run the full unit and integration test suite
cd neurowealth-vault
cargo test --verbose

# 4. Run a fuzz target (requires nightly toolchain)
cargo +nightly fuzz run share_accounting_invariants -- -runs=1000 -max_total_time=120

# 5. Verify access control classification
bash ../scripts/check-access-control.sh

# 6. Verify public function auth gate
bash ../scripts/check-pub-fn-auth.sh
```

> **Prerequisites:** Rust stable + nightly toolchains, `cargo-fuzz` (`cargo install cargo-fuzz`), Stellar CLI (version pinned in `.stellar-version`).

---

## 10. Auditor Coordination

| Field | Value |
| :--- | :--- |
| Expected engagement start | `<DATE>` |
| Expected duration | `<WEEKS>` |

### Finding severity definitions

Auditors are encouraged to use the following severity scale for consistency:

| Severity | Definition |
| :--- | :--- |
| **Critical** | Direct loss of user funds or complete contract takeover |
| **High** | Significant financial loss, privilege escalation, or bypass of core security controls |
| **Medium** | Indirect financial impact, griefing, or weakening of a security control |
| **Low** | Minor issues, best-practice deviations, or issues requiring unlikely preconditions |
| **Info** | Observations, suggestions, or out-of-scope items noted for completeness |

---

*Document prepared by the NeuroWealth engineering team — last updated 2026-08-24.*
