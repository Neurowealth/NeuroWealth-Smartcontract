# E2E Devnet Artifact Lifecycle

This document explains what `scripts/e2e-devnet.sh` generates, which outputs
are safe to delete, and how to restore from uploaded CI artifacts.

---

## What Gets Generated

Running `e2e-devnet.sh` produces two categories of output.

### Local CLI identities

The script creates two Stellar CLI keypairs in your local identity store
(`~/.config/stellar/identity/` or equivalent):

| Identity name | Purpose |
|---------------|---------|
| `e2e-deployer` | Deploys the USDC token and vault contracts; holds the owner/agent role |
| `e2e-user` | Test user that deposits and withdraws USDC |

These are ephemeral keypairs generated fresh on every run if not already
present. They are funded automatically via Stellar Friendbot (testnet only).

> **On-chain state created by these identities cannot be deleted** — Stellar
> transactions are final. The testnet is periodically reset by the SDF, which
> clears all on-chain state.

### Local artifact files

All file outputs land in `scripts/e2e-artifacts/`:

| File | Contents | Disposable? |
|------|----------|-------------|
| `contract_id.txt` | Deployed vault contract address | ✅ Yes |
| `usdc_token_id.txt` | Test USDC token address | ✅ Yes |
| `deployer_address.txt` | Deployer/agent public key | ✅ Yes |
| `user_address.txt` | Test user public key | ✅ Yes |
| `tx_<scenario>.txt` | Raw CLI output for each scenario | ✅ Yes |
| `events.txt` | Contract events fetched after all scenarios | ✅ Yes |
| `storage_layout_diff.txt` | DataKey storage layout compatibility comparison report | ✅ Yes |
| `pre_upgrade_snapshot.json` | Pre-upgrade getter state snapshot for smoke verification | ✅ Yes |
| `post_upgrade_smoke.txt` | Post-upgrade read-only getter sweep verification report | ✅ Yes |
| `summary.txt` | Pass/fail summary | ✅ Yes |
| `full_output.log` | Complete stdout (written by CI only) | ✅ Yes |
| `.gitkeep` | Keeps the directory tracked in git | ❌ No |

All files except `.gitkeep` are regenerated on the next run. Deleting them is
always safe.

---

## Retention Policy

| Output category | Retention | Safe to delete? |
|-----------------|-----------|-----------------|
| `scripts/e2e-artifacts/*.txt` / `*.log` | Until next run or manual cleanup | ✅ Yes |
| Local CLI identities (`e2e-deployer`, `e2e-user`) | Until `e2e-restore.sh` is run | ✅ Yes |
| CI-uploaded artifacts (14-day GitHub retention) | Auto-expired by GitHub | N/A |
| On-chain testnet contracts / transactions | Until SDF testnet reset | ❌ Cannot delete |

---

## Cleaning Up After a Run

Remove **all** local state (identities + artifacts):

```bash
./scripts/e2e-restore.sh
```

Remove identities but **keep** artifact files (useful for post-mortem analysis):

```bash
./scripts/e2e-restore.sh --keep-artifacts
```

Remove artifact files only, leaving identities in place:

```bash
rm -f scripts/e2e-artifacts/*.txt scripts/e2e-artifacts/*.log
```

---

## Restoring from CI Artifacts

When an E2E run fails in CI, GitHub uploads the artifact bundle as
`e2e-devnet-artifacts` (14-day retention).

**Steps to restore locally:**

1. Go to the GitHub Actions run page.
2. Download `e2e-devnet-artifacts.zip` from the *Artifacts* section.
3. Unzip into `scripts/e2e-artifacts/`:
   ```bash
   unzip e2e-devnet-artifacts.zip -d scripts/e2e-artifacts/
   ```
4. Inspect the failure:
   ```bash
   # Overall summary
   cat scripts/e2e-artifacts/summary.txt

   # Full log
   cat scripts/e2e-artifacts/full_output.log

   # Per-scenario output
   cat scripts/e2e-artifacts/tx_<scenario>.txt
   ```
5. Look up the contract on the explorer using the saved contract ID:
   ```bash
   CONTRACT=$(cat scripts/e2e-artifacts/contract_id.txt)
   echo "https://stellar.expert/explorer/testnet/contract/$CONTRACT"
   ```

---

## Re-running Against an Existing Deployment

If identities are still present (i.e., `e2e-restore.sh` has not been run) and
`scripts/e2e-artifacts/contract_id.txt` exists, you can re-run individual
`stellar contract invoke` commands from `full_output.log` or `tx_*.txt` without
redeploying.

> The vault and USDC contracts on testnet remain valid until the next SDF
> network reset. Check [status.stellar.org](https://status.stellar.org) for
> upcoming resets.

---

## Starting Fresh

If identities are stale or Friendbot-funded accounts have been swept by a
testnet reset, run:

```bash
./scripts/e2e-restore.sh   # remove old identities and artifacts
./scripts/e2e-devnet.sh    # deploy fresh and run all scenarios
```

The script will generate and fund new keypairs automatically.
