# NeuroWealth Mainnet Deployment Checklist

This document outlines the mandatory formal verification steps, configuration parameters, and emergency readiness checks that must be successfully executed before and during the deployment of the `NeuroWealthVault` smart contract to the Stellar Mainnet.

---

## 📋 Table of Contents

1. [Key Management Setup (Separate Owner & Agent Keys)](#1-key-management-setup-separate-owner--agent-keys)
2. [Initialization Parameters & Deployment Verification](#2-initialization-parameters--deployment-verification)
3. [Administrative Caps & Deposit Limits Configuration](#3-administrative-caps--deposit-limits-configuration)
4. [Blend Pool Integration & Address Verification](#4-blend-pool-integration--address-verification)
5. [DEX Pool Integration & Address Verification](#5-dex-pool-integration--address-verification)
6. [Emergency Procedures & Pause Drill Runbook](#6-emergency-procedures--pause-drill-runbook)
7. [Upgrade & Governance Multisig Plan](#7-upgrade--governance-multisig-plan)
8. [Third-Party Security Audit & Formal Sign-off](#8-third-party-security-audit--formal-sign-off)

---

## 1. Key Management Setup (Separate Owner & Agent Keys)

To uphold the principle of least privilege and prevent single points of failure, **the Owner and Agent keys must be completely separate and generated independently**.

### 🔍 Security Context

- **Owner (Cold/Multisig):** Holds sensitive administrative capabilities like contract pausing, unpausing, TVL/cap changes, and contract upgrades. This key represents a high-value target and should be kept securely offline (e.g., hardware wallet or multi-signature account setup).
- **AI Agent (Hot):** Used by the automated backend system to submit frequent rebalancing signals and assets updates (`rebalance` and `update_total_assets`). Since it lives in a hot environment (server memory), it faces a higher compromise risk.
- **The Risk:** If the Owner and Agent keys are the same, a compromise of the AI agent backend would immediately compromise the ownership and control of the entire contract, enabling an attacker to upgrade the contract or block users from withdrawing.

### 📝 Actionable Checklist

- [ ] **Generate Independent Keypairs:** Ensure that the Owner address ($G_{owner}$) and Agent address ($G_{agent}$) are completely separate and do not share any key material.
- [ ] **Establish Key Storage Environments:**
  - Owner private key: Saved in a secure offline HSM, multi-sig hardware wallet, or Stellar Multisig account.
  - Agent private key: Stored in a secure environment variables vault (e.g., AWS Secrets Manager, Vault, Supabase Vault) with restricted read access.
- [ ] **Pre-Launch Address Verification:**
  - Query testnet/mainnet deploy keys:
    ```bash
    owner_addr=$(stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_owner)
    agent_addr=$(stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_agent)
    ```
  - Verify that `$owner_addr` != `$agent_addr`.

> **Automated check** — `scripts/verify-deployment.sh` asserts owner address, agent address, and owner ≠ agent separation in one command:
>
> ```bash
> VAULT_CONTRACT_ID=C... NETWORK=mainnet \
>   OWNER_ADDRESS=G... AGENT_ADDRESS=G... AGENT_SECRET_KEY=S... \
>   USDC_TOKEN_ADDRESS=G... \
>   ./scripts/verify-deployment.sh
> ```

---

## 2. Initialization Parameters & Deployment Verification

Initialization of the `NeuroWealthVault` uses a cryptographic commitment to protect against front-running. The deployer key must immediately call `initialize` after deployment.

### 🔍 Security Context

- The contract verifies that the `deployer` address combined with the deployed `salt` cryptographically reproduces the contract address and requires deployer's authentication (`deployer.require_auth()`).
- After successful initialization, the temporary deployer key has no administrative powers.

### 📝 Actionable Checklist

- [ ] **Deployer Key Separation:** Generate a clean, single-use `deployer` keypair. Fund it with enough native XLM to cover deployment fees.
- [ ] **Parameter Configuration Verification:** Double-check the mainnet initialization arguments before submitting the transaction:
  - `--deployer`: Address of the temporary deployer key.
  - `--owner`: Verified cold/multisig owner address.
  - `--agent`: Verified AI agent address.
  - `--usdc_token`: Official Stellar Mainnet USDC Token address (`GBBD67VQMKA676776SGXN6776...` - verify on Stellar Expert).
  - `--salt`: A securely generated 32-byte hash.
- [ ] **Execute and Discard Deployer Key:**
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source deployer \
    --network mainnet \
    -- \
    initialize \
    --deployer $DEPLOYER_ADDRESS \
    --owner $OWNER_ADDRESS \
    --agent $AGENT_ADDRESS \
    --usdc_token $USDC_TOKEN_ADDRESS \
    --salt $SALT
  ```
- [ ] **Post-Init Read Verification:**
  - Run `get_owner` to confirm it returns `$OWNER_ADDRESS`.
  - Run `get_agent` to confirm it returns `$AGENT_ADDRESS`.
  - Run `get_usdc_token` to confirm it returns `$USDC_TOKEN_ADDRESS`.
- [ ] **Discard Deployer Key:** Erase/discard the temporary deployer key. It should never be reused.

---

## 3. Administrative Caps & Deposit Limits Configuration

To limit financial risk and systemic exposure during the initial stages of launch, safety caps must be configured.

### 🔍 Security Context

- **TVL Cap:** Prevents the vault from accepting more than a specific aggregate deposit, limiting the overall capital at risk.
- **User Deposit Cap:** Limits exposure per single user, preventing whales from dominating the pool and mitigating risks of heavy individual exposure.
- **Deposit Limits (Min/Max):** Enforces transaction thresholds (minimum of 1 USDC to protect against dust attacks and first-depositor inflation attacks).

### 📝 Actionable Checklist

- [ ] **Initial TVL Cap Setup:** Determine the conservative launch phase TVL cap (e.g., $100,000 USD represented as `100000000000` base units - 7 decimals).
- [ ] **Initial User Deposit Cap Setup:** Determine the initial limit per user (e.g., $5,000 USD represented as `5000000000` base units).
- [ ] **Enforce Caps:** Call `set_caps` via the Owner key:
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_caps \
    --user_deposit_cap 5000000000 \
    --tvl_cap 100000000000
  ```
- [ ] **Set Transaction Limits:** Call `set_deposit_limits` (e.g., min 1 USDC, max 5,000 USDC):
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_deposit_limits \
    --min 1000000 \
    --max 5000000000
  ```
- [ ] **Verify Settings:** Query getters `get_tvl_cap`, `get_user_deposit_cap`, `get_min_deposit`, and `get_max_deposit` to verify correctness.

> **Automated check** — `scripts/verify-deployment.sh` fetches all four caps and compares them against your declared expected values:
>
> ```bash
> VAULT_CONTRACT_ID=C... NETWORK=mainnet \
>   OWNER_ADDRESS=G... AGENT_ADDRESS=G... AGENT_SECRET_KEY=S... \
>   USDC_TOKEN_ADDRESS=G... \
>   EXPECTED_TVL_CAP=100000000000 \
>   EXPECTED_USER_DEPOSIT_CAP=5000000000 \
>   EXPECTED_MIN_DEPOSIT=1000000 \
>   EXPECTED_MAX_DEPOSIT=5000000000 \
>   ./scripts/verify-deployment.sh
> ```
>
> The script exits non-zero if any cap does not match or if an `EXPECTED_*` variable is missing.

---

## 4. Blend Pool Integration & Address Verification

The NeuroWealth AI agent deploys assets into Blend lending pools. Registering the correct, verified mainnet contract address for Blend is critical.

### 🔍 Security Context

- Deploying to an incorrect or malicious pool address can lead to instant loss of principal funds.
- While the contract's `set_blend_pool` method performs interface probing by calling `balance()` to confirm the contract conforms to the expected Blend pool structure, this does not guarantee the address belongs to the genuine Blend protocol.

### 📝 Actionable Checklist

- [ ] **Retrieve Official Blend Registries:** Match the Blend mainnet pool address against:
  - Official Blend Protocol documentation.
  - Verified GitHub repository resources or Blend UI configurations.
  - The verified on-chain deployment logs on a block explorer (Stellar Expert).
- [ ] **Perform Interface/State Verification:** Call the pool's read methods directly on the mainnet RPC to check pool parameters.
- [ ] **Register Verified Blend Pool:** Call `set_blend_pool` using the Owner key:
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_blend_pool \
    --owner $OWNER_ADDRESS \
    --pool_address $VERIFIED_BLEND_POOL_ADDRESS
  ```
- [ ] **Read Verification:** Query `get_blend_pool` on the vault to confirm the registered address matches the verified Blend pool address.

> **Automated check** — set `BLEND_POOL_ADDRESS` and `scripts/verify-deployment.sh` will assert that `get_blend_pool()` returns that exact address (not null):
>
> ```bash
> VAULT_CONTRACT_ID=C... NETWORK=mainnet \
>   OWNER_ADDRESS=G... AGENT_ADDRESS=G... AGENT_SECRET_KEY=S... \
>   USDC_TOKEN_ADDRESS=G... \
>   BLEND_POOL_ADDRESS=C... \
>   ./scripts/verify-deployment.sh
> ```

---

## 5. DEX Pool Integration & Address Verification

The NeuroWealth AI agent deploys assets into DEX liquidity pools for active trading strategies. Registering the correct, verified mainnet contract address for the target DEX pool is critical.

### 🔍 Security Context

- Deploying to an incorrect, unverified, or malicious pool address could result in permanent loss of funds or slippage exploitation.
- Interface validation alone does not confirm that the DEX pool is genuine or safe. Address verification against trusted registries is mandatory before deployment.

### 📝 Actionable Checklist

- [ ] **Retrieve Official DEX Registries:** Match the DEX pool mainnet address against official protocol documentation and verified on-chain deployment logs.
- [ ] **Perform Interface/State Verification:** Verify DEX pool parameters and liquidity depth.
- [ ] **Register Verified DEX Pool:** Call `set_dex_pool` using the Owner key:
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_dex_pool \
    --owner $OWNER_ADDRESS \
    --pool_address $VERIFIED_DEX_POOL_ADDRESS
  ```
- [ ] **Read Verification:** Query `get_dex_pool` on the vault to confirm the registered address matches the verified DEX pool address.

> **Automated check** — set `DEX_POOL_ADDRESS` and `scripts/verify-deployment.sh` will assert that `get_dex_pool()` returns that exact address (not null):
>
> ```bash
> VAULT_CONTRACT_ID=C... NETWORK=mainnet \
>   OWNER_ADDRESS=G... AGENT_ADDRESS=G... AGENT_SECRET_KEY=S... \
>   USDC_TOKEN_ADDRESS=G... \
>   DEX_POOL_ADDRESS=C... \
>   ./scripts/verify-deployment.sh
> ```

---

## 6. Emergency Procedures & Pause Drill Runbook

Before deploying to Mainnet, the team must run an on-chain Pause Drill on Testnet to guarantee emergency mechanisms function as intended and operators are trained in execution.

For detailed incident response procedures, refer to:
- [Owner-Compromise Response Runbook](../SECURITY.md#owner-compromise-response-runbook)
- [Agent-Key Compromise Runbook](AGENT_KEY_COMPROMISE_RUNBOOK.md) - Detection, timelock rotation, and user communication for agent-key incidents

### 🔍 Security Context

- The `pause` function blocks all deposits, withdrawals, and rebalances during an active hack, protocol compromise, or market emergency.
- Operators must be familiar with the latency, transaction structure, and consequences of pausing/unpausing the contract.

### 📝 Execution Plan (Pause Drill Runbook)

1. **Trigger Emergency Pause:** Owner invokes `pause` on testnet.
   ```bash
   stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- pause --owner $OWNER_ADDRESS
   ```
2. **Verify State Updates:** Confirm `is_paused()` returns `true`.
3. **Verify Security Invariants (Deposits):** Attempt a test deposit.
   - _Expected Result:_ The transaction MUST fail and revert with `VaultError::Paused` (Error Code `35`).
4. **Verify Security Invariants (Withdrawals):** Attempt a test withdrawal.
   - _Expected Result:_ The transaction MUST fail and revert with `VaultError::Paused` (Error Code `35`).
5. **Verify Security Invariants (Rebalances):** Attempt an AI agent rebalance trigger.
   - _Expected Result:_ The transaction MUST fail and revert with `VaultError::Paused` (Error Code `35`).
6. **Trigger Resume (Unpause):** Owner invokes `unpause`.
   ```bash
   stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- unpause --owner $OWNER_ADDRESS
   ```
7. **Verify Resumed Operation:** Verify that `is_paused()` returns `false`, and normal deposits, withdrawals, and rebalances execute successfully.

- [ ] **Testnet Drill Completed successfully:** Sign off on the drill.

### 📝 Game-Day Drill: Alert-to-Pause RTO Measurement (Devnet)

The pause drill above proves the mechanism works; this drill measures how fast
the *organization* can use it. It exercises the full detection → response
chain — monitoring alert fires, on-call is paged, `emergency_pause` lands
on-chain — and records the elapsed time as the incident Recovery Time
Objective (RTO) baseline for mainnet.

**Cadence:** run once before mainnet launch, then quarterly. Rotate which
operator is on-call so every keyholder has executed a pause under time
pressure at least once.

**Roles:**

- **Drill conductor** — injects the signal, keeps the timestamp log, does not
  assist the responder.
- **On-call responder** — receives the page and executes the runbook exactly
  as they would in a real incident (no pre-warming of CLI sessions or keys).

**Procedure (all timestamps in UTC, captured by the conductor):**

1. **T0 — Inject simulated exploit signal.** Without pre-announcing the exact
   time, the conductor triggers one of the monitoring alerts from
   [monitoring.md](monitoring.md) against the devnet deployment — e.g. submit
   transactions that trip `withdrawal_spike`, or fire the alert rule directly
   in the monitoring stack with a `[DRILL]` prefix.
2. **T1 — Alert fired.** Timestamp when the monitoring system actually emits
   the page/notification.
3. **T2 — Responder acknowledged.** Timestamp when the on-call operator acks
   the page.
4. **T3 — `emergency_pause` submitted.** Responder runs, from the standard
   runbook (no shortcuts):
   ```bash
   stellar contract invoke --id $DEVNET_VAULT_CONTRACT_ID --source owner --network testnet -- emergency_pause --owner $OWNER_ADDRESS
   ```
5. **T4 — Pause confirmed on-chain.** Timestamp of the ledger that includes
   the transaction; verify `is_paused()` returns `true` and the
   `EmergencyPausedEvent` was emitted.
6. **Debrief.** Compute `RTO = T4 − T0`. Record every blocker encountered
   (key retrieval friction, missing docs, RPC issues, alert routing delays)
   and file an issue for each. Unpause the devnet vault and confirm normal
   operation resumes.

**RTO target (mainnet): `T4 − T0 ≤ 15 minutes`, with `T3 − T2 ≤ 5 minutes`.**
A drill exceeding the target must be re-run after the identified blockers are
fixed; do not sign off mainnet readiness on a failed drill.

**Drill log** (append one row per drill; keep raw timestamp notes in the
incident-response log):

| Date (UTC) | Responder | T0 signal | T1 alert | T2 ack | T3 submitted | T4 confirmed | RTO (T4−T0) | Target met | Blockers / follow-ups |
| ---------- | --------- | --------- | -------- | ------ | ------------ | ------------ | ----------- | ---------- | --------------------- |
|            |           |           |          |        |              |              |             |            |                       |

- [ ] **Game-day drill executed in devnet** with all five timestamps captured in the log above.
- [ ] **Measured RTO recorded and ≤ 15-minute target** (re-run after fixes if not).
- [ ] **Blockers filed as issues** and assigned owners.

---

## 7. Upgrade & Governance Multisig Plan

The Owner key holds upgrade privileges. To secure the contract against single-key compromise or loss, the owner account should be configured with multi-signature security.

### 🔍 Security Context

- Soroban allows upgrading contract code. An attacker possessing the owner key could upload a malicious WASM binary to hijack user funds.
- The instant `upgrade()` entrypoint has been replaced by a two-step timelocked flow (Issue #316): `schedule_upgrade` → wait `UPGRADE_TIMELOCK_LEDGERS` (17,280 ledgers ≈ 24 h) → `execute_upgrade`, with `cancel_upgrade` as the escape hatch. The timelock is the last line of defence if the multisig itself is compromised — it converts an instant code swap into a 24-hour, publicly observable event.
- Stellar natively supports multi-signature operations directly at the account level through account signer thresholds and weights.

### 📝 Actionable Checklist

- [ ] **WASM Hash Verification Gate:** Before calling `schedule_upgrade` on mainnet, the WASM hash **must match** a CI-published hash from a signed git tag release build.
  - Verify CI workflow ran on the intended release tag (e.g., `v2.1.0`).
  - Confirm the CI build artifact WASM hash is recorded in `CHANGELOG.md` under that version.
  - Run `stellar contract install` on mainnet and verify the returned hash **byte-for-byte matches** the CI-published hash.
  - Record the matching hash and CI job URL in the release ticket for audit trail.
  - _Rationale:_ This gate ensures the exact bytecode deployed to mainnet was built from a tagged, reviewable commit in git and is not locally-modified or compromised.
- [ ] **Configure Owner Multisig Account:** Configure the mainnet Owner address with multiple signers (e.g., 2-of-3 or 3-of-5 setup).
  - **Threshold Settings:**
    - Low threshold (e.g., 1): For triggering simple operations or `pause()` (allows fast emergency response with a single hot trigger key).
    - Medium threshold (e.g., 2 or 3): For configuring caps, setting Blend pools, and `unpause()`.
    - High threshold (e.g., 3): For calling `schedule_upgrade()` and `execute_upgrade()` (requires multi-party consensus to push new code).
  - Keep `cancel_upgrade()` reachable at a **low** threshold. It is the escape hatch during the timelock window and must not be blocked by an unavailable co-signer.
- [ ] **Document Signer Distribution:** Ensure keys are distributed securely across key parties using hardware wallets (e.g., Ledger).
- [ ] **Upgrade Verification Procedure:** Ensure any future WASM upgrades are:
  - Built inside a deterministic environment (e.g., Docker container with exact Rust toolchain versions).
  - Checked against WASM size limits using standard optimization tools (`wasm-opt -Oz`).
  - Signatures collected offline from all co-signers before broadcast.

### 📝 Timelocked Upgrade Verification Drill (Testnet)

The timelock must be exercised end-to-end on testnet before mainnet deployment. The unit tests in `neurowealth-vault/contracts/vault/src/tests/test_upgrade_timelock.rs` cover the gates by advancing the simulated ledger, but they cannot verify the WASM swap or the `Version` bump — the dummy hash they schedule is not installed on-chain. Only a real network run proves the full cycle.

Set `TESTNET_VAULT_CONTRACT_ID`, `OWNER_ADDRESS`, and `NEW_WASM_HASH` (the hex hash returned by `stellar contract install`) before starting.

**Part A — `get_pending_upgrade` returns the correct hash and expiry**

- [ ] **A1. Confirm a clean starting state:** with nothing scheduled, the getter must return `null`.
  ```bash
  stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- get_pending_upgrade
  ```
- [ ] **A2. Record the current ledger sequence** from RPC (`getLatestLedger`) — call it `L`.
- [ ] **A3. Schedule the upgrade.**
  ```bash
  stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- schedule_upgrade --owner $OWNER_ADDRESS --new_wasm_hash $NEW_WASM_HASH
  ```

  - _Expected Result:_ Success, and an `UpgradeScheduledEvent` (topic `upg_sched`) carrying `new_wasm_hash` and `effective_ledger`.
- [ ] **A4. Verify the pending state:** re-run `get_pending_upgrade`.
  - _Expected Result:_ `(wasm_hash, effective_ledger)` where `wasm_hash` byte-for-byte equals `$NEW_WASM_HASH` and `effective_ledger ≈ L + 17280`. Confirm the delta is exactly `UPGRADE_TIMELOCK_LEDGERS`, not a shortened test value.
- [ ] **A5. Verify the "only one pending" guard:** call `schedule_upgrade` again with any hash.
  - _Expected Result:_ MUST fail with `VaultError::TimelockAlreadyPending` (Error Code `48`).
- [ ] **A6. Verify the execute gate holds before expiry:** call `execute_upgrade` immediately.
  ```bash
  stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- execute_upgrade --owner $OWNER_ADDRESS
  ```

  - _Expected Result:_ MUST fail with `VaultError::TimelockNotExpired` (Error Code `50`). Confirm `get_version()` is unchanged and the deployed code still behaves as the old build — a pending proposal must have no effect on the running contract.

**Part B — `cancel_upgrade` clears the pending state**

- [ ] **B1. Cancel the proposal scheduled in Part A.**
  ```bash
  stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- cancel_upgrade --owner $OWNER_ADDRESS
  ```

  - _Expected Result:_ Success, and an `UpgradeCancelledEvent` (topic `upg_cncl`) carrying the cancelled hash.
- [ ] **B2. Verify both storage keys are cleared:** `get_pending_upgrade` MUST return `null` (`PendingUpgradeHash` and `UpgradeTimelockExpiry` are both removed).
- [ ] **B3. Verify cancel is idempotency-guarded:** call `cancel_upgrade` again.
  - _Expected Result:_ MUST fail with `VaultError::NoTimelockPending` (Error Code `49`).
- [ ] **B4. Verify `execute_upgrade` is also blocked after cancel.**
  - _Expected Result:_ MUST fail with `VaultError::NoTimelockPending` (Error Code `49`).
- [ ] **B5. Verify the escape hatch survives a pause:** schedule again, `pause()` the vault, then call `cancel_upgrade`.
  - _Expected Result:_ `schedule_upgrade` and `execute_upgrade` are pause-gated and MUST fail with `VaultError::Paused` (Error Code `35`), but `cancel_upgrade` MUST succeed. Unpause afterwards.

**Part C — full cycle: schedule → wait out the timelock → execute → verify version bump**

- [ ] **C1. Record `get_version()`** before starting — call it `V`.
- [ ] **C2. Install the new WASM on testnet** and capture its hash.
  ```bash
  stellar contract install --wasm target/wasm32-unknown-unknown/release/neurowealth_vault.wasm --source owner --network testnet
  ```

  - A hash that is not installed on-chain will trap at `execute_upgrade` time, _after_ the 24-hour wait. Verify installation before scheduling.
- [ ] **C3. Schedule the upgrade** with that hash and note `effective_ledger` from `get_pending_upgrade`.
- [ ] **C4. Wait out the real timelock.** Testnet has no ledger fast-forward: the 17,280 ledgers take ≈ 24 hours of wall-clock time. **Do not** shorten `UPGRADE_TIMELOCK_LEDGERS` for this drill — the point is to confirm the mainnet constant. Poll `get_pending_upgrade` during the window and confirm the hash never changes.
- [ ] **C5. Execute once the current ledger sequence `>= effective_ledger`.**
  ```bash
  stellar contract invoke --id $TESTNET_VAULT_CONTRACT_ID --source owner --network testnet -- execute_upgrade --owner $OWNER_ADDRESS
  ```

  - _Expected Result:_ Success, and an `UpgradedEvent` (topic `upgraded`) with `old_version` = `V` and `new_version` = `V + 1`.
- [ ] **C6. Verify the version bump:** `get_version()` MUST return `V + 1`.
- [ ] **C7. Verify the pending state was cleared by execution:** `get_pending_upgrade` MUST return `null`, and a fresh `schedule_upgrade` MUST be accepted (no leftover `TimelockAlreadyPending`).
- [ ] **C8. Verify storage survived the upgrade:** re-check `get_total_assets()`, `get_total_shares()`, `get_owner()`, `get_agent()`, and a sample `get_shares(user)` against the values recorded before C5.
- [ ] **C9. Run the release's migration entrypoint** if it ships one, then re-run the state checks in C8. The current contract has no `migrate()`; see [UPGRADE_MIGRATION.md](UPGRADE_MIGRATION.md) for the pattern a future release would follow.
- [ ] **C10. Sign off:** record the drill's ledger numbers, WASM hashes, and transaction hashes in the release ticket.

> **Note:** operational runbooks for scheduling, monitoring, and executing a _production_ upgrade live in [UPGRADE_MIGRATION.md](UPGRADE_MIGRATION.md). This drill is the pre-mainnet verification that the timelock itself behaves correctly. The agent-rotation timelock (Issue #317) follows the same shape — see the _Agent Update Timelock_ section of [ARCHITECTURE.md](../ARCHITECTURE.md).

---

## 9. Harvest Cooldown & Circuit-Breaker Configuration

The `harvest()` entry-point reuses the same `MinRebalanceInterval` / `LastRebalanceLedger`
cooldown mechanism as `rebalance()`. An incorrectly set cooldown can either allow runaway
harvesting (too low) or lock the AI agent out of yield compounding (too high). The
circuit-breaker (`MaxConsecutiveFailures`) automatically suspends the agent when the configured
threshold of consecutive protocol failures is reached, preventing a stuck external pool from
draining gas indefinitely.

### 🔍 Security Context

- **Harvest cooldown** — `harvest()` checks `LastRebalanceLedger` before executing. If the
  elapsed ledgers since the last rebalance or harvest is below `MinRebalanceInterval`, the call
  panics with `VaultError::RebalanceCooldownActive` (Error Code `43`). A zero interval disables
  the guard entirely.
- **Circuit-breaker** — after `MaxConsecutiveFailures` successive protocol errors the agent is
  suspended. The default (`DEFAULT_MAX_CONSECUTIVE_FAILURES`) is applied when the vault was
  initialized before the circuit-breaker feature shipped. Setting the threshold to `0` disables
  the breaker (not recommended in production).

### 📝 Actionable Checklist

- [ ] **Choose a harvest cooldown interval.** A typical starting point is 720 ledgers (≈ 1 hour).
      Set it with:
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_rebalance_cooldown \
    --interval 720
  ```
- [ ] **Verify the cooldown is stored correctly:**
  ```bash
  stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_rebalance_cooldown
  # Expected: 720 (or whatever value you configured)
  ```
- [ ] **Verify `harvest()` respects the cooldown.** Immediately after a harvest, attempt a second
      call from the agent key.
  - _Expected Result:_ MUST fail with `VaultError::RebalanceCooldownActive` (Error Code `43`).
- [ ] **Choose a circuit-breaker threshold.** A value of `3`–`5` is recommended; this trips
      automatic suspension after that many consecutive protocol failures without blocking normal
      operations during transient outages.
  ```bash
  stellar contract invoke \
    --id $VAULT_CONTRACT_ID \
    --source owner \
    --network mainnet \
    -- \
    set_max_consecutive_failures \
    --threshold 5
  ```
- [ ] **Verify the circuit-breaker threshold:**
  ```bash
  stellar contract invoke --id $VAULT_CONTRACT_ID --network mainnet -- get_max_consecutive_failures
  # Expected: 5
  ```
- [ ] **Confirm the circuit-breaker trips correctly on testnet.** Simulate consecutive harvest
      failures (e.g., by draining the Blend pool mock) and confirm that after `threshold` failures the
      agent is suspended and subsequent calls revert.

> **Automated check** — add `EXPECTED_REBALANCE_COOLDOWN` and `EXPECTED_MAX_CONSECUTIVE_FAILURES`
> to the `verify-deployment.sh` invocation to assert both values in one step:
>
> ```bash
> VAULT_CONTRACT_ID=C... NETWORK=mainnet \
>   OWNER_ADDRESS=G... AGENT_ADDRESS=G... AGENT_SECRET_KEY=S... \
>   USDC_TOKEN_ADDRESS=G... \
>   EXPECTED_REBALANCE_COOLDOWN=720 \
>   EXPECTED_MAX_CONSECUTIVE_FAILURES=5 \
>   ./scripts/verify-deployment.sh
> ```

---

## 8. Third-Party Security Audit & Formal Sign-off

No smart contract should be deployed on-chain without an independent security audit and formal sign-off.

### 📝 Actionable Checklist

- [ ] **Run Pre-Audit Scans & Tests:** Confirm all unit tests pass locally:
  - Run `cargo test` and verify 100% success rate on comprehensive tests.
- [ ] **Complete Third-Party Professional Audit:**
  - Secure a professional smart contract auditing firm (e.g., CertiK, Zellic, OpenZeppelin, Halborn).
  - Resolve and fix any identified vulnerabilities (High, Medium, Low, Informational).
  - Receive final audit sign-off documentation.
- [ ] **Verify Findings In Codebase:** Verify that critical fixes (such as `withdraw_all()` balance protection, and `update_total_assets` balance checks) are compile-ready and active.
- [ ] **Final Sign-Off:** Gather signatures from the lead developers, security auditors, and product leads before deploying the finalized bytecode.
