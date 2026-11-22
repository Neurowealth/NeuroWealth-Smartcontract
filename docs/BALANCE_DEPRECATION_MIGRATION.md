# Balance Storage Deprecation — Migration Guide

## Overview

`DataKey::Balance(Address)` is the **first variant** (discriminant 0) of the vault's
`DataKey` enum. It is **deprecated** — no production code reads or writes it — but it
**cannot be removed** because doing so would shift the discriminant values of every
subsequent variant, breaking on-chain storage compatibility across upgrades.

All user balances are now derived from `Shares(user)` and the current exchange rate:

```text
balance = (shares * sharePrice) / PRECISION
```

## Why Balance Is Retained

Soroban serializes enum variants by their position (discriminant). The current layout:

| Discriminant | Variant            | Status                                    |
| ------------ | ------------------ | ----------------------------------------- |
| 0            | `Balance(Address)` | **Deprecated** — retained for layout only |
| 1            | `Shares(Address)`  | Active — canonical per-user storage       |
| 2+           | Other variants     | Active                                    |

Removing `Balance(Address)` would make `Shares(Address)` shift from discriminant 1 to
discriminant 0, causing all existing `Shares` storage entries to become unreadable.

## Migration Path

### What Changed

1. **Deposit flow**: Previously wrote `DataKey::Balance(user)` directly. Now mints
   `Shares(user)` and the balance is derived from the share-to-asset exchange rate.
2. **Withdraw flow**: Previously read `DataKey::Balance(user)`. Now reads `Shares(user)`
   and computes the withdrawable amount.
3. **Getters**: `get_balance(user)` now computes `shares * totalAssets / totalShares`
   instead of reading from storage.

### For Integrators

No action required. The `get_balance(user)` API remains unchanged — the derivation
happens internally. Existing off-chain indexers that read events will continue to
work since `DepositEvent` and `WithdrawEvent` emit both `amount` and `shares`.

### For Upgrades

When deploying a new contract version:

1. **Do NOT remove `DataKey::Balance` from the enum.** The variant must remain at
   discriminant 0 with the same `Address` payload type.
2. **Do NOT rename `DataKey::Balance`.** The serialized form depends on the variant
   name for Soroban's `contracttype` macro.
3. You MAY add new variants after the existing ones (appending to the enum).
4. Run the verification script before and after upgrade to ensure storage integrity.

## Verification

### Automated Check

Run the deprecation verification script:

```bash
bash scripts/check-balance-deprecation.sh
```

This script verifies:

- `DataKey::Balance(Address)` is declared and marked deprecated
- No production code paths read or write `DataKey::Balance`
- All test/fuzz references use the mock `TokenDataKey::Balance`, not the vault's
- `Balance` remains the first enum variant (discriminant 0)
- No getters read from the Balance key

### Manual Checklist

- [ ] `DataKey::Balance(Address)` is the first variant in the enum
- [ ] The variant has a `/// Deprecated` doc comment
- [ ] `grep -rn 'DataKey::Balance' neurowealth-vault/contracts/vault/src/lib.rs`
      shows only the enum definition (no reads/writes in function bodies)
- [ ] `get_balance()` derives from shares, not storage
- [ ] All deposits mint shares via `DataKey::Shares`
- [ ] `test_upgrade_compatibility.rs` passes (discriminant stability test)
- [ ] `scripts/check-balance-deprecation.sh` exits 0

### What to Look For in Code Reviews

Any new code that introduces `DataKey::Balance` reads or writes should be **rejected**
in code review with the following rationale:

> DataKey::Balance is deprecated. User balances must be derived from
> Shares(user) and the exchange rate. See docs/BALANCE_DEPRECATION_MIGRATION.md.

## Future Removal

In a hypothetical future where Soroban supports storage schema versioning or
migration entrypoints, `DataKey::Balance` could be removed. Until then:

- The variant stays in the enum at discriminant 0
- The `BalanceDeprecation.sol` placeholder in `contracts/migrations/` is retained
  as documentation of the migration concept
- The test in `test_balance_deprecation.rs` validates layout stability on every
  CI run

---

## User-Facing Recovery Guide: TTL-Lapsed Balance Entries

### What Is TTL and Why Does It Matter?

Soroban persistent storage entries have a **Time-To-Live (TTL)** — a ledger count after
which the entry is automatically removed if not touched (read or written). When a user's
`Shares(user)` entry's TTL lapses:

- The share count is deleted from the ledger.
- Subsequent calls to `get_balance(user)` will return `0` (no shares stored).
- The funds are **not lost** — the contract's total asset count remains accurate —
  but the user's _individual_ balance entry needs restoration.

This is a **storage maintenance feature**, not a loss of funds. It prevents unlimited
ledger bloat from dormant accounts.

### Detection: How Do I Know My Balance Entry Expired?

#### Symptom 1: Balance Shows Zero After Inactivity

You had shares in the vault, but after several weeks of no interaction:

```bash
stellar contract invoke --id $VAULT_ID -- get_balance --user $YOUR_ADDRESS
# Returns: 0 USDC
```

But you remember depositing funds. This likely means the `Shares(user)` entry's TTL lapsed.

#### Symptom 2: Transaction Logs Show Deposits But Zero Balance

Check the vault's transaction history (via RPC or indexer):

- You see a successful `DepositEvent` for your address.
- Your `get_shares(your_address)` returns `0`.
- Vault's `get_total_shares()` and `get_total_assets()` are still non-zero.

**This is the telltale sign:** Your entry expired but the vault's global accounting is intact.

#### Symptom 3: Partial TTL Decay (If You Still Hold Other Entries)

You have multiple addresses or made deposits at different times. Some addresses show a
balance, others show zero. This suggests:

- Active addresses: TTLs are being touched by recent transactions.
- Inactive addresses: TTLs have lapsed from lack of activity.

### Restoration: How Do I Recover My Entry?

#### Method 1: Call `touch_user_ttl` (Recommended)

The simplest and most direct method: any address can call `touch_user_ttl` on behalf
of a user to extend the `Shares(user)` entry's TTL back to its maximum (typically
52,560,000 ledgers ≈ ~6 months on Stellar).

**How it works:**

1. The contract checks if `Shares(user)` exists in persistent storage.
2. If it does, the TTL is extended to the configured maximum.
3. If it does NOT exist (entry is fully gone), the function returns `false`.
4. You receive `true` on success, `false` if the entry is irrecoverable.

**Command:**

```bash
stellar contract invoke \
  --id $VAULT_ID \
  --source $PAYER_KEY \
  --network mainnet \
  -- \
  touch_user_ttl \
  --user $YOUR_ADDRESS
```

**Result:**

```
txn hash: ...
Result: Ok(Bool)
bool: true  ← Entry exists and TTL extended. Balance is now accessible.
     false ← Entry is gone; vault has no record of shares. (See Method 2 below.)
```

**Gas cost:** ~100–200 Stroops (negligible, <$0.01 USD).

#### Method 2: If `touch_user_ttl` Returns False (Entry is Gone)

If `touch_user_ttl` returns `false`, the entry has been **fully pruned** from the ledger —
there is no in-contract record to restore. This can happen if:

- The TTL expired and was never touched before the contract was upgraded or state
  was compacted.
- The user never actually deposited (transaction failed but was recorded elsewhere).

**Recovery steps:**

1. **Verify your shares are actually gone** — check the `DepositEvent` history:

   ```bash
   # Query RPC for all DepositEvent entries for your address
   # If you see deposits but shares are zero, you need a manual audit.
   ```

2. **Contact the NeuroWealth team** with:
   - Your wallet address (`$YOUR_ADDRESS`)
   - The date(s) you remember depositing
   - Transaction hashes (if available) of your deposit attempts
   - The output of `touch_user_ttl --user $YOUR_ADDRESS`

3. **The team will:**
   - Cross-reference on-chain events (DepositEvent, WithdrawEvent, RebalanceEvent)
   - Check `get_total_assets()` and `get_total_shares()` consistency
   - Verify whether your funds are accounted for in the vault's global tally
   - If the vault's math is intact but your entry is missing, a contract upgrade may
     be needed to implement a recovery entrypoint or to restore the entry via a
     one-time migration transaction.

### Prevention: How Do I Keep My Balance Entry From Expiring?

#### Option 1: Regularly Interact with Your Account

Any transaction that touches your `Shares(user)` entry **resets the TTL to maximum**:

- `deposit()` — adds to your shares
- `withdraw()` — burns your shares
- `set_user_strategy()` — stores your strategy preference (doesn't affect shares TTL directly,
  but confirms account interaction)
- `rebalance()` — affects global accounts, so the AI agent's deposits/withdrawals touch
  many users' TTLs incidentally

**Practical:** Interact with your vault account at least once every ~3 months to keep
the TTL fresh.

#### Option 2: Let the AI Agent Handle It

If you have deposits in the vault and the AI agent is actively rebalancing funds between
protocols, those rebalance operations will **indirectly touch** your shares entry as a side
effect. So active users whose funds are being deployed are unlikely to experience TTL
expiry.

**Practical:** Active strategies (Balanced, Growth) result in frequent touches. Conservative
strategies or very small deposits might not trigger agent activity often enough.

#### Option 3: Pre-Emptively Extend Your TTL

If you plan to be inactive for a while:

1. Call `touch_user_ttl` a few days before you go dark.
2. This buys you an additional ~6 months.
3. Mark a calendar reminder to touch it again when you return.

**Command:**

```bash
stellar contract invoke \
  --id $VAULT_ID \
  --source $PAYER_KEY \
  --network mainnet \
  -- \
  touch_user_ttl \
  --user $YOUR_ADDRESS
# Result: true (entry touched and refreshed)
```

### FAQ

**Q: Will my funds be lost if my balance entry expires?**
A: No. The vault's global `TotalAssets` and `TotalShares` counters remain accurate.
Your share count is a logical entry that can be restored; the underlying funds are
always accounted for at the contract level.

**Q: Can I restore my balance if the entry is completely gone?**
A: If `touch_user_ttl` returns `false` and there are no recent deposits in the event
history, the entry cannot be restored by the contract alone. Contact support for
manual audit and recovery options.

**Q: How long until my entry expires?**
A: Typically ~6 months (52,560,000 ledgers) of complete inactivity. Stellar RPC can
report the current TTL via `getLedgerEntry` queries for a specific storage key.

**Q: Do I pay gas to call `touch_user_ttl`?**
A: Yes, a small fee (~100–200 Stroops, <$0.01 USD). The transaction must be signed and
submitted like any other Stellar transaction.

**Q: Can I call `touch_user_ttl` from a different address?**
A: Yes. `touch_user_ttl` requires no authorization — anyone can extend anyone else's TTL.
This allows the AI agent or a monitoring system to proactively refresh dormant accounts.

**Q: Will the balance entry ever be required again?**
A: No. `DataKey::Balance(Address)` is permanently deprecated. All user accounting is
derived from `Shares(user)` and the exchange rate. The old `Balance` key is retained
only for storage layout stability during upgrades.

## References

- `neurowealth-vault/contracts/vault/src/lib.rs` — `DataKey` enum definition and
  `touch_user_ttl` implementation
- `neurowealth-vault/contracts/vault/src/tests/test_upgrade_compatibility.rs` — discriminant stability tests
- `scripts/check-balance-deprecation.sh` — automated verification script
- `docs/UPGRADE_MIGRATION.md` — general upgrade migration guide
- `docs/monitoring.md` — operational monitoring of TTL and storage entries
