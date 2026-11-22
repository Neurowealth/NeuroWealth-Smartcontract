# Fuzz Corpus Documentation

This directory contains fuzz testing harnesses for the NeuroWealth Vault contract.

## Fuzz Targets

### 1. `deposit_withdraw_sequence`
**Purpose**: Tests random deposit/withdraw sequences against the vault.
**Input format**: 3-byte chunks where:
- Byte 0: Operation selector (0=deposit, 1=withdraw)
- Bytes 1-2: Amount selector (u16 LE)
**Invariants checked**: User shares/balances non-negative, user shares ≤ total shares

### 2. `rebalance_transitions`
**Purpose**: Tests protocol switching (none → blend → none → dex → none) to catch state-inconsistency bugs during rebalance transitions.
**Input format**: 4-byte chunks where:
- Byte 0: Operation selector (0=deposit, 1=rebalance, 2=withdraw)
- Bytes 1-2: Amount/apy selector (u16 LE)
- Byte 3: Target protocol index (0=none, 1=blend, 2=dex)
**Invariants checked**: Standard vault invariants after each operation

### 3. `rounding_boundaries`
**Purpose**: Tests rounding at boundary conditions including minimum/maximum deposits, partial withdrawals, and round-trip deposit/withdraw sequences.
**Input format**: 3-byte chunks where:
- Byte 0: Operation selector (0=boundary deposit, 1=boundary withdraw, 2=round-trip, 3=multiple small deposits)
- Bytes 1-2: Amount selector (u16 LE)
**Invariants checked**: Standard vault invariants after each operation

### 4. `share_accounting_invariants`
**Purpose**: Tests share-accounting invariants across multiple users with deposit/withdraw/asset-update sequences.
**Input format**: 4-byte chunks where:
- Byte 0: Operation selector (0=deposit, 1=withdraw, 2=round-trip, 3=cross-user deposit)
- Byte 1: User index selector
- Bytes 2-3: Amount selector (u16 LE)
**Invariants checked**:
1. `total_assets >= total_deposits` (yield non-negative)
2. `user_shares <= total_shares` for all users
3. `user_balance <= total_assets` for all users
4. `user_balance <= expected_balance` (proportional share)
5. Exchange rate >= 1.0

### 5. `agent_update_timelock`
**Purpose**: Exercises random propose/confirm/cancel sequences for agent updates while advancing the ledger to stress the timelock logic.
**Input format**: 4-byte chunks where:
- Byte 0: Operation selector (0=propose update, 1=confirm update, 2=cancel update, 3=advance ledger)
- Bytes 1-2: Ledger/amount selector (u16 LE)
- Byte 3: Agent selector (0-3)
**Invariants checked**:
1. At most one pending agent update exists at a time
2. Confirmation only succeeds after the timelock delay has elapsed
3. Cancellation is always allowed while a proposal is pending
4. Scheduled `effective_ledger` is always in the future

### 6. `harvest_sequence` (Issue #671)
**Purpose**: Extends `deposit_withdraw_sequence` to interleave `harvest()` and `rebalance()` calls, ensuring the harvest function handles arbitrary inputs without panics or state corruption under adversarial sequences.
**Input format**: 4-byte chunks where:
- Byte 0: Operation selector (0=deposit, 1=withdraw, 2=harvest, 3=rebalance)
- Bytes 1-2: Amount/min_out selector (u16 LE)
- Byte 3: Target protocol index for rebalance (0=none, 1=blend, 2=dex)
**Invariants checked**:
1. Standard vault invariants after every successful operation (shares/balances non-negative, user ≤ total)
2. `TotalAssets` does not decrease after a `harvest()` call (yield can only be non-negative)
3. No unexpected panics from any harvest code path

### 7. `withdrawal_queue_invariants` (Issue #777)
**Location**: `share-math/fuzz/` — a standalone cargo-fuzz project over the pure
`share-math` crate (`share_math::queue::QueueModel`), so it builds without the
Soroban contract crate.
**Purpose**: Fuzzes share accounting for a FIFO withdrawal queue: shares are
ceil-escrowed on `queue_withdrawal`, returned on cancel, and burned/refunded on
`process_withdrawal_queue` at the fulfilment-time exchange rate.
**Input format**: first byte selects `max_size` (1–8); then 4-byte chunks:
- Byte 0: Operation (0=deposit, 1=queue, 2=cancel, 3=process, 4=yield, 5=loss, 6=recall liquidity)
- Byte 1: User / request-id / process-limit selector
- Bytes 2-3: Amount selector (u16 LE)
**Invariants checked**:
1. `total_shares == sum(free_shares + escrowed_shares)`
2. Per-user escrow equals the shares locked by that user's pending requests
3. No negative balances; `0 <= idle_assets <= total_assets`
4. Pending requests never exceed `max_size`
5. Strict FIFO — no pending request older than a fulfilled one
6. Payout never exceeds requested assets; assets leaving the vault equal assets paid
7. Fulfilment never lowers the exchange rate for remaining holders
8. Cancel restores the exact escrow, is owner-only, and cannot repeat; terminal requests are immutable

Run: `cargo +nightly fuzz run --fuzz-dir share-math/fuzz withdrawal_queue_invariants`.
The same harness runs on stable via `cargo test -p share-math queue`.

## Running Fuzz Tests

```bash
# Run all fuzz targets
cargo +nightly fuzz run <target_name>

# Run with specific duration
cargo +nightly fuzz run <target_name> -- -max_total_time=60

# Run with specific memory limit
cargo +nightly fuzz run <target_name> -- -max_len=1024

# View corpus
ls fuzz/artifacts/<target_name>/

# Minimize corpus
cargo +nightly fuzz tmin <target_name> <crash_file>
```

## Known Allowed Panics

The following panics are expected and documented in the vault contract:
- `Error(Contract, #37)` — AmountMustBePositive
- `Error(Contract, #38)` — BelowMinimumDeposit
- `Error(Contract, #39)` — MaximumDepositExceeded
- `Error(Contract, #40)` — ExceedsUserDepositCap
- `Error(Contract, #41)` — ExceedsTvlCap
- `Error(Contract, #6)`  — SharesToMintMustBePositive
- `Error(Contract, #7)`  — InsufficientLiquidity
- `Error(Contract, #8)`  — InsufficientShares
- `Error(Contract, #10)` — SharesToBurnMustBePositive
- `Error(Contract, #11)` — InsufficientSharesForRequestedAmount
- `Error(Contract, #17)` — UnsupportedProtocol
- `Error(Contract, #35)` — Paused
