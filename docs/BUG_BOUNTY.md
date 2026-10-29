# NeuroWealth Bug Bounty Program

> **Status:** Active — Pre-Mainnet  
> **Last updated:** 2026-08-24  
> **Program managed by:** NeuroWealth Security Team

We reward security researchers who responsibly disclose vulnerabilities in the
NeuroWealth smart contract system. Our goal is a safe mainnet launch, and your
help makes that possible.

---

## Table of Contents

1. [Scope](#scope)
2. [Out of Scope](#out-of-scope)
3. [Severity Rubric](#severity-rubric)
4. [Reporting Channel](#reporting-channel)
5. [Safe-Harbor Terms](#safe-harbor-terms)
6. [Response SLAs](#response-slas)
7. [Payout Process](#payout-process)
8. [Disclosure Policy](#disclosure-policy)

---

## Scope

The following assets are **in scope** for the bug bounty:

### Smart Contracts (Primary Scope)

| Path | Description |
|------|-------------|
| `neurowealth-vault/contracts/vault/src/lib.rs` | Core vault contract (deposit, withdraw, rebalance, upgrade, pause logic) |
| `neurowealth-vault/contracts/vault/src/topics.rs` | Event topic constants |
| All deployed contract addresses listed in the latest [Mainnet Deployment Runbook](../scripts/MAINNET_DEPLOYMENT_RUNBOOK.txt) | On-chain instances |

### Repository / Off-Chain Components (Secondary Scope)

| Path | Description |
|------|-------------|
| `agent/src/` | AI agent backend (event listener, intent parser, yield comparison) |
| `packages/vault-client/src/` | TypeScript vault client library |
| `scripts/deploy-*.sh` | Deployment scripts (key hygiene, initialization ordering) |

### What We Care About Most

- **Share-price manipulation** — any technique that artificially inflates or
  deflates the vault's `total_assets` / `total_shares` ratio to steal value
  from other depositors.
- **Authentication bypass** — calling owner-only, agent-only, or user-only
  functions without the required authorization.
- **Unauthorized fund extraction** — any path that moves USDC to an address
  that did not deposit it.
- **Upgrade hijacking** — bypassing the upgrade timelock or forging a
  `schedule_upgrade` / `execute_upgrade` call.
- **Pause bypass** — executing a paused-blocked function while the vault is
  paused.
- **Reentrancy** — despite CEI pattern enforcement, any novel cross-contract
  reentrant path that breaks invariants.
- **Integer overflow / underflow** — arithmetic bugs in share or asset
  accounting that checked-math should catch but might not.
- **Front-running / MEV** — exploitable ordering of initialize, deposit, or
  upgrade transactions in the Stellar mempool.

---

## Out of Scope

The following are **not eligible** for bounty rewards:

- Theoretical or speculative attacks with no working proof-of-concept.
- Issues in third-party protocols (Blend, Stellar DEX) unless the vault's
  integration amplifies the impact.
- Bugs already reported or currently being fixed in an open issue or PR.
- Bugs in test files (`src/tests/`) or fuzz targets (`fuzz/`) that do not
  reflect production contract behavior.
- Social-engineering attacks against team members.
- Denial-of-service that only affects testnet or devnet.
- Front-end / UI bugs that are cosmetic and do not affect funds.
- Issues requiring physical access to a developer's machine.
- Informational-only findings with no exploitable impact.
- Planned / acknowledged risks already documented in
  [`SECURITY.md`](../SECURITY.md) (e.g., Blend utilization liquidity risk).

---

## Severity Rubric

We use a four-tier severity system aligned with industry standards
(Immunefi / HackerOne).

### Critical — Up to **$50,000**

Direct, on-chain theft or permanent loss of user funds without requiring any
privileged key.

**Example bug classes:**

| Class | Example |
|-------|---------|
| Share-price manipulation | Inflate `total_assets` via `update_total_assets` to drain other users on withdrawal |
| Auth bypass | Call `rebalance()` or `execute_upgrade()` as an arbitrary address |
| Unauthorized withdrawal | Extract USDC to a non-depositor address without their auth |
| Upgrade hijack | Execute `execute_upgrade()` before the timelock expires, or bypass the owner-auth check |
| Reentrancy theft | Cross-contract reentrant call that double-mints shares or double-withdraws USDC |

**Criteria:** Funds at risk, exploitable on mainnet, no trusted actor required.

---

### High — Up to **$10,000**

Severe impact on vault integrity or user funds requiring a single compromised
or malicious trusted actor (owner or agent).

**Example bug classes:**

| Class | Example |
|-------|---------|
| Privilege escalation | Agent can call owner-only functions (e.g., `set_tvl_cap`) |
| Pause bypass | Paused function executes a state-changing operation despite the paused flag |
| Forced lock-up | Owner can permanently brick withdrawals beyond the documented pause mechanism |
| Cap bypass | Depositing more than `user_deposit_cap` or `tvl_cap` in a single tx |
| Agent over-reporting | `update_total_assets` reports more than on-chain balance without triggering the solvency check |

**Criteria:** High impact, but typically requires a compromised key or
specific race condition.

---

### Medium — Up to **$2,500**

Moderate impact, exploitable under specific conditions, or degraded security
guarantees.

**Example bug classes:**

| Class | Example |
|-------|---------|
| Griefing / DoS | Any account can lock the vault into a state requiring owner intervention |
| Rounding manipulation | Systematic rounding abuse to drain value from the vault over many transactions |
| Event spoofing | Emit misleading events that cause off-chain agents to take incorrect action |
| TTL expiry abuse | Deliberately expire another user's `Shares` storage entry to cause data loss |
| Cooldown bypass | Call `rebalance()` / `harvest()` more frequently than configured |

**Criteria:** Real impact but not direct fund loss in a single transaction.

---

### Low — Up to **$500**

Minor issues that violate documented security properties but have limited
practical exploitability.

**Example bug classes:**

| Class | Example |
|-------|---------|
| Access-control gap | Non-critical function callable by wrong role without real-world impact |
| Missing event | State-changing function that silently omits an event that indexers rely on |
| Input validation | Edge-case input (e.g., `amount = 0`) not rejected with the correct error code |
| Documentation mismatch | SECURITY.md or ARCHITECTURE.md describes behavior that differs from code |

**Criteria:** Low exploitability or impact confined to off-chain tooling.

---

## Reporting Channel

**Primary and Exclusive:** Use [GitHub Private Vulnerability Reporting](https://github.com/Neurowealth/NeuroWealth-Smartcontract/security/advisories/new)
via the **"Report a vulnerability"** button on the repository's Security tab.
This channel is encrypted end-to-end and keeps all details private until a
coordinated disclosure date is agreed.

> **Note:** The legacy email address (`security@neurowealth.io`) is permanently unavailable (domain NXDOMAIN) and has been deprecated. GitHub Private Vulnerability Reporting is the **only** active private channel. Do not send emails or attempt to contact via defunct addresses.

**Do NOT** open a public GitHub issue for a security vulnerability. Doing so
will disqualify the report from bounty eligibility.

### Report Template

Please include the following in your report:

```
**Severity (your assessment):** Critical / High / Medium / Low

**Affected component:** e.g., lib.rs → execute_upgrade()

**Vulnerability description:**
<clear explanation of the bug>

**Attack scenario:**
<step-by-step description of how an attacker would exploit this>

**Impact:**
<what an attacker gains: funds stolen, vault bricked, etc.>

**Proof of concept:**
<minimal Rust test or transaction sequence that demonstrates the bug>

**Suggested fix (optional):**
<your recommendation>
```

---

## Safe-Harbor Terms

NeuroWealth is committed to working with security researchers in good faith.
We will **not** pursue legal action against researchers who:

1. Discover and report vulnerabilities via the process described in this
   document.
2. Act in good faith and do not exploit the vulnerability beyond what is
   necessary to produce a minimal proof of concept.
3. Do not access, modify, or exfiltrate user data beyond what is required
   to demonstrate the vulnerability.
4. Do not perform denial-of-service attacks, social engineering, or physical
   attacks against NeuroWealth infrastructure or personnel.
5. Do not publicly disclose the vulnerability before the coordinated
   disclosure deadline (see [Disclosure Policy](#disclosure-policy)).

If you inadvertently access user funds or data while researching a
vulnerability, stop immediately, include it in your report, and we will
work with you to assess the impact without penalty.

**Testing environment:** All testing should be performed on Stellar
**testnet** or **devnet**. Testing directly against mainnet contracts may
disqualify your report and could expose you to legal risk.

---

## Response SLAs

| Milestone | Target |
|-----------|--------|
| Initial acknowledgement | **48 hours** of receiving the report |
| Triage and severity assignment | **5 business days** |
| Fix developed and reviewed | **14 business days** (Critical / High) |
| Fix developed and reviewed | **30 business days** (Medium / Low) |
| Patch deployed to testnet | **7 days** after fix review |
| Patch deployed to mainnet | Dependent on timelock (24 h) + deployment schedule |
| Bounty paid | **7 business days** after mainnet deployment confirmation |
| Coordinated public disclosure | **90 days** after initial report (may be shortened by mutual agreement) |

If a fix requires more time than the SLA allows, we will communicate progress
proactively and agree on an updated timeline with the reporter.

---

## Payout Process

1. **Confirmation:** We send a written confirmation of bounty eligibility and
   the approved severity tier.
2. **Validation:** Reporter provides wallet address (Stellar or EVM).
3. **Payment:** Bounties are paid in **USDC** on the Stellar network.
4. **Tax:** Reporters are responsible for any applicable taxes in their
   jurisdiction.
5. **Acknowledgement:** With permission, we will credit the reporter in the
   patch release notes and our security hall of fame.

Payout amounts are determined by:

- Severity tier (see rubric above).
- Quality and completeness of the report.
- Novelty of the finding.
- Whether a working proof-of-concept was provided.

Duplicate reports (same root cause already reported by another researcher)
receive a reduced or no bounty, at our discretion.

---

## Disclosure Policy

- We follow a **90-day coordinated disclosure** window from the date we
  acknowledge your report.
- We will work with you to agree on a disclosure date that is as early as
  possible while protecting users.
- If the vulnerability is actively being exploited, we reserve the right to
  accelerate disclosure and deployment.
- We will credit all eligible reporters in our public post-mortem unless
  anonymity is requested.

---

*This policy is subject to change. Material changes will be announced via the
repository changelog and a repository announcement.*

*See also: [`SECURITY.md`](../SECURITY.md) for the full trust model, threat
analysis, and owner-compromise runbook.*
