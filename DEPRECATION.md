# Deprecation & Migration Guide

This document outlines deprecated functions and their migration paths.

## set_limits (Deprecated)

### Status
- **Deprecated in**: Version 1.x
- **Will be removed in**: Next major version bump

### What it did
`set_limits(min, max)` set both `UserDepositCap` and `TvlCap` in a single call.

### Migration Path

Use one of these two functions instead:

#### Option 1: Set both caps atomically (recommended)
```rust
pub fn set_caps(env: Env, user_deposit_cap: i128, tvl_cap: i128)
```
This is the direct replacement that maintains atomic behavior.

#### Option 2: Set limits separately
```rust
pub fn set_tvl_cap(env: Env, cap: i128)
pub fn set_user_deposit_cap(env: Env, cap: i128)
pub fn set_deposit_limits(env: Env, min: i128, max: i128)
```

### Parameter Mapping

| set_limits param | set_caps param    | Alternative           |
|------------------|-------------------|-----------------------|
| min              | user_deposit_cap  | set_user_deposit_cap  |
| max              | tvl_cap           | set_tvl_cap           |

### Example

**Old code (deprecated):**
```rust
client.set_limits(&1_000_000, &100_000_000_000);
```

**New code (recommended):**
```rust
client.set_caps(&1_000_000, &100_000_000_000);
```

### Timeline
- Current version: Function still works but is marked deprecated
- Next major version: Function will be removed entirely
- Callers should migrate now to avoid breakage

### Notes
- `set_limits` returns `Result<(), VaultError>` while `set_caps` returns `()`
- All parameter semantics remain identical
- `set_caps` is atomic and preferred for coordinated updates
