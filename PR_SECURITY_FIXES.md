# Security Fixes: Yield Sniping, Decrease Cap, and Zero-Assets Protection

This PR addresses three critical security vulnerabilities in the NeuroWealth Vault contract:

- **#975**: Yield sniping protection via non-zero default MinHoldingPeriod
- **#974**: Owner-configured decrease cap instead of caller-supplied argument
- **#973**: Deposit rejection when TotalShares > 0 && TotalAssets == 0

## Changes

### 1. Yield Sniping Protection (#975)

**Problem**: Yield reaches the share price only when the agent calls `update_total_assets`, which can raise TotalAssets by any amount up to the real balance in one step. With MinHoldingPeriod defaulting to 0, anyone watching the mempool or the agent's schedule could deposit just before the update and withdraw right after, capturing yield accrued over a period they were not invested.

**Solution**: 
- Set default `MinHoldingPeriod` to 120 ledgers (~10 minutes) in `initialize()`
- This prevents same-block deposit/withdraw cycles that would allow yield sniping
- Owner can still adjust via `set_min_holding_period` if needed

**Files modified**:
- `neurowealth-vault/contracts/vault/src/lib.rs`: Added default MinHoldingPeriod in initialize

### 2. Decrease Cap Configuration (#974)

**Problem**: `update_total_assets(agent, new_total, allow_decrease, max_decrease_bps)` capped a loss report at `max(max_decrease_bps, 100)` bps of the old total. The cap was a caller-supplied argument, so the caller could pass 10_000 and report a 100% loss in one call. The only safeguard was the owner co-sign requirement.

**Solution**:
- Added `DataKey::MaxDecreaseBps` for owner-configured storage
- Added `set_max_decrease_bps()` and `get_max_decrease_bps()` functions
- Default cap: 1000 bps (10%)
- Hard ceiling: 10_000 bps (100%) enforced in setter
- Updated `update_total_assets()` to read from storage and ignore caller-supplied argument
- Caller-supplied argument is now prefixed with `_` to indicate it's unused

**Files modified**:
- `neurowealth-vault/contracts/vault/src/lib.rs`: Added MaxDecreaseBps storage key, setter/getter, updated update_total_assets
- `neurowealth-vault/contracts/vault/src/types.rs`: Added MaxDecreaseBps variant

### 3. Zero-Assets-With-Shares Protection (#973)

**Problem**: `convert_to_shares_internal` used a bootstrap 1:1 rate when `total_shares == 0 || total_assets == 0`. If shares were outstanding but assets were zero (e.g., after a full loss report, or dust shares remaining after rounding), a new depositor would get `amount` shares while existing holders, whose shares were worth nothing, would immediately own a pro-rata slice of the new deposit.

**Solution**:
- Added new error `VaultError::DepositRejectedZeroAssetsWithShares`
- Updated `convert_to_shares_internal()` to reject deposits when `total_shares > 0 && total_assets == 0`
- This prevents dilution attack where new depositors get 1:1 shares while existing holders' shares are worthless

**Files modified**:
- `neurowealth-vault/contracts/vault/src/errors.rs`: Added DepositRejectedZeroAssetsWithShares error
- `neurowealth-vault/contracts/vault/src/lib.rs`: Added check in convert_to_shares_internal

### 4. Documentation

**SECURITY.md Updates**:
- Added Section 8: Yield Sniping Protection (Issue #975)
- Added Section 9: Decrease Cap Configuration (Issue #974)
- Added Section 10: Zero-Assets-With-Shares Protection (Issue #973)

Each section documents the threat, mitigation, and impact.

### 5. Tests

Added comprehensive test coverage for all three fixes:

- `test_zero_assets_with_shares.rs`: Tests for deposit rejection when TotalShares > 0 && TotalAssets == 0
  - Deposit rejection in zero-assets state
  - Bootstrap state (both zero) still works
  - Normal operation (both positive) still works
  - batch_deposit also protected

- `test_stored_decrease_cap.rs`: Tests for owner-configured decrease cap
  - Default cap is 1000 bps (10%)
  - Owner can configure cap
  - Hard ceiling of 10_000 bps enforced
  - update_total_assets uses stored cap, ignores caller argument
  - Decrease exceeding stored cap is rejected
  - Decrease within stored cap is allowed
  - Only owner can set cap

- `test_default_holding_period.rs`: Tests for default MinHoldingPeriod
  - Default is 120 ledgers (~10 minutes)
  - Withdrawal blocked before period elapses
  - Withdrawal succeeds after period elapses
  - Owner can adjust period
  - Yield sniping attack prevented

All test modules registered in `tests/mod.rs`.

## Testing

Run the new tests:
```bash
cd neurowealth-vault/contracts/vault
cargo test test_zero_assets_with_shares
cargo test test_stored_decrease_cap
cargo test test_default_holding_period
```

## Security Impact

- **#975**: Attackers must wait at least 120 ledgers between deposit and withdrawal, making yield sniping unprofitable for typical reporting intervals
- **#974**: Even with owner co-sign, a single transaction cannot report a loss exceeding the configured cap (default 10%). Repeated calls cannot chain past the limit without additional owner authorization
- **#973**: New depositors cannot be diluted by existing worthless shares. Vault requires explicit recovery action before accepting new deposits after a total loss event

## Backward Compatibility

- Existing vaults will need to be upgraded to benefit from these fixes
- The default MinHoldingPeriod will apply to newly initialized vaults
- Existing vaults can set MinHoldingPeriod via `set_min_holding_period` without upgrade
- MaxDecreaseBps defaults to 1000 bps if not set, so existing vaults are protected on first decrease call
- The zero-assets-with-shares check applies to all vaults immediately after upgrade

## Checklist

- [x] Code changes implemented
- [x] SECURITY.md updated with threat documentation
- [x] Tests added for all three fixes
- [x] Test modules registered in mod.rs
- [ ] Manual testing completed
- [ ] Code review completed

Closes #975, #974, #973
