//! Shared test utilities for NeuroWealth Vault tests
//!
//! # Token Mock
//!
//! [`token::TestToken`] / [`TestTokenClient`] is the **single canonical mock token**
//! used across all unit and integration tests.  No other test module should define
//! its own token contract — import this one via `use super::utils::*;` instead.
//! Closes issue #288.

extern crate std;

use soroban_sdk::{
    contract, contractimpl, contracttype, testutils::Address as _, Address, BytesN, Env, Map,
    Symbol, TryFromVal, Val, Vec,
};

// Re-export so each submodule only needs `use super::utils::*;`
pub use crate::{NeuroWealthVault, NeuroWealthVaultClient};
pub use soroban_sdk::testutils::Events;

// ============================================================================
// SIMPLE TEST TOKEN CONTRACT
// ============================================================================

#[contracttype]
enum TokenDataKey {
    Balance(Address),
    Allowance(Address, Address),
    AllowanceExpiration(Address, Address),
}

#[derive(Clone)]
#[contracttype]
enum BlendMockDataKey {
    Supplied(Address),
    /// Configurable max supply limit (0 = no limit, use requested amount)
    MaxSupplyLimit,
    /// Configurable max withdraw limit per transaction (0 = no limit)
    MaxWithdrawLimit,
    /// Override the return value of submit_with_allowance without changing the
    /// actual transferred amount. Used to test balance-delta accounting.
    ReportedSupplyOverride,
    /// The asset most recently supplied via `submit_with_allowance`, so
    /// `get_reserve`/`get_positions` can report real Blend-shaped data for
    /// it instead of needing an explicit asset parameter (real Blend keys
    /// reserves by asset; this mock only ever tracks one at a time, which
    /// matches every test's actual usage).
    TrackedAsset,
}

#[derive(Clone)]
#[contracttype]
enum DexMockDataKey {
    /// Liquidity supplied by a provider for a given asset.
    Supplied(Address),
    /// Configurable max supply limit (0 = no limit, use requested amount)
    MaxSupplyLimit,
    /// Configurable max withdraw limit per transaction (0 = no limit)
    MaxWithdrawLimit,
    /// Override the return value of add_liquidity without changing actual transfer amount.
    /// When set, add_liquidity reports this value instead of actual_amount.
    ReportedSupplyOverride,
}

pub mod token {
    use super::*;

    #[contract]
    pub struct TestToken;

    #[contractimpl]
    impl TestToken {
        pub fn mint(env: Env, to: Address, amount: i128) {
            let balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(to.clone()))
                .unwrap_or(0);
            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(to), &(balance + amount));
        }

        pub fn transfer(env: Env, from: Address, to: Address, amount: i128) {
            from.require_auth();
            assert!(amount > 0, "amount must be positive");

            let from_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(from.clone()))
                .unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");

            let to_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(to.clone()))
                .unwrap_or(0);

            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(from), &(from_balance - amount));
            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(to), &(to_balance + amount));
        }

        pub fn balance(env: Env, owner: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&TokenDataKey::Balance(owner))
                .unwrap_or(0)
        }

        pub fn approve(
            env: Env,
            from: Address,
            spender: Address,
            amount: i128,
            expiration_ledger: u32,
        ) {
            from.require_auth();
            assert!(amount >= 0, "amount must be non-negative");

            env.storage().persistent().set(
                &TokenDataKey::Allowance(from.clone(), spender.clone()),
                &amount,
            );
            env.storage().persistent().set(
                &TokenDataKey::AllowanceExpiration(from, spender),
                &expiration_ledger,
            );
        }

        pub fn allowance(env: Env, from: Address, spender: Address) -> i128 {
            let expiration: u32 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::AllowanceExpiration(
                    from.clone(),
                    spender.clone(),
                ))
                .unwrap_or(0);

            if expiration < env.ledger().sequence() {
                return 0;
            }

            env.storage()
                .persistent()
                .get(&TokenDataKey::Allowance(from, spender))
                .unwrap_or(0)
        }

        pub fn allowance_expiration(env: Env, from: Address, spender: Address) -> u32 {
            env.storage()
                .persistent()
                .get(&TokenDataKey::AllowanceExpiration(from, spender))
                .unwrap_or(0)
        }

        pub fn transfer_from(env: Env, spender: Address, from: Address, to: Address, amount: i128) {
            spender.require_auth();
            assert!(amount > 0, "amount must be positive");

            let allowance = Self::allowance(env.clone(), from.clone(), spender.clone());
            assert!(allowance >= amount, "insufficient allowance");

            let from_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(from.clone()))
                .unwrap_or(0);
            assert!(from_balance >= amount, "insufficient balance");

            let to_balance: i128 = env
                .storage()
                .persistent()
                .get(&TokenDataKey::Balance(to.clone()))
                .unwrap_or(0);

            env.storage().persistent().set(
                &TokenDataKey::Balance(from.clone()),
                &(from_balance - amount),
            );
            env.storage()
                .persistent()
                .set(&TokenDataKey::Balance(to), &(to_balance + amount));
            env.storage().persistent().set(
                &TokenDataKey::Allowance(from, spender.clone()),
                &(allowance - amount),
            );
        }
    }
}

pub use token::{TestToken, TestTokenClient};

pub mod blend {
    use super::*;

    #[contract]
    pub struct MockBlendPool;

    #[contractimpl]
    impl MockBlendPool {
        pub fn submit_with_allowance(
            env: Env,
            from: Address,
            spender: Address,
            _to: Address,
            requests: Vec<crate::BlendRequest>,
        ) -> i128 {
            assert_eq!(requests.len(), 1, "expected one request");
            let request = requests.get(0).unwrap();
            assert_eq!(request.request_type, 0, "expected supply request");

            let token_client = TestTokenClient::new(&env, &request.address);
            let allowance = token_client.allowance(&spender, &env.current_contract_address());
            assert!(
                allowance >= request.amount,
                "expected allowance before pool pull"
            );

            // Check for configured supply shortfall limit
            let max_supply_limit: i128 = env
                .storage()
                .persistent()
                .get(&BlendMockDataKey::MaxSupplyLimit)
                .unwrap_or(0);

            // Calculate actual amount to supply (respecting limits)
            let actual_amount = if max_supply_limit < 0 {
                0
            } else if max_supply_limit > 0 {
                core::cmp::min(request.amount, max_supply_limit)
            } else {
                request.amount
            };

            env.storage()
                .persistent()
                .set(&BlendMockDataKey::TrackedAsset, &request.address);

            if actual_amount > 0 {
                token_client.transfer_from(
                    &env.current_contract_address(),
                    &spender,
                    &env.current_contract_address(),
                    &actual_amount,
                );

                let total_supplied: i128 = env
                    .storage()
                    .persistent()
                    .get(&BlendMockDataKey::Supplied(request.address.clone()))
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &BlendMockDataKey::Supplied(request.address),
                    &(total_supplied + actual_amount),
                );
            }

            from.clone().require_auth();

            let override_amount: Option<i128> = env
                .storage()
                .persistent()
                .get(&BlendMockDataKey::ReportedSupplyOverride);
            override_amount.unwrap_or(actual_amount)
        }

        /// Sets a max supply limit to simulate pool shortfall scenarios.
        /// 0 = no limit (default behavior)
        pub fn set_max_supply_limit(env: Env, limit: i128) {
            env.storage()
                .persistent()
                .set(&BlendMockDataKey::MaxSupplyLimit, &limit);
        }

        /// Makes submit_with_allowance report `reported` as the return value
        /// regardless of how much was actually transferred. Used to test that
        /// the vault measures outcome via balance-delta rather than trusting the
        /// pool's return value.
        pub fn set_reported_supply_amount(env: Env, reported: i128) {
            env.storage()
                .persistent()
                .set(&BlendMockDataKey::ReportedSupplyOverride, &reported);
        }

        /// Sets a max withdraw limit to simulate withdrawal failures/stuck funds.
        /// 0 = no limit; negative = reject all withdrawals.
        pub fn set_max_withdraw_limit(env: Env, limit: i128) {
            env.storage()
                .persistent()
                .set(&BlendMockDataKey::MaxWithdrawLimit, &limit);
        }

        pub fn submit(
            env: Env,
            from: Address,
            _spender: Address,
            to: Address,
            requests: Vec<crate::BlendRequest>,
        ) {
            from.require_auth();

            assert_eq!(requests.len(), 1, "expected one request");
            let request = requests.get(0).unwrap();

            let token_client = TestTokenClient::new(&env, &request.address);
            let pool_balance = token_client.balance(&env.current_contract_address());

            match request.request_type {
                1 => {
                    // Withdraw request (type 1)
                    let amount_to_withdraw = core::cmp::min(request.amount, pool_balance);

                    // Check for configured withdrawal limit (to simulate stuck funds scenarios)
                    let max_withdraw_limit: i128 = env
                        .storage()
                        .persistent()
                        .get(&BlendMockDataKey::MaxWithdrawLimit)
                        .unwrap_or(0);

                    let actual_withdraw = if max_withdraw_limit < 0 {
                        0
                    } else if max_withdraw_limit > 0 {
                        core::cmp::min(amount_to_withdraw, max_withdraw_limit)
                    } else {
                        amount_to_withdraw
                    };

                    if actual_withdraw > 0 {
                        token_client.transfer(
                            &env.current_contract_address(),
                            &to,
                            &actual_withdraw,
                        );

                        // Update supplied tracking
                        let total_supplied: i128 = env
                            .storage()
                            .persistent()
                            .get(&BlendMockDataKey::Supplied(request.address.clone()))
                            .unwrap_or(0);
                        env.storage().persistent().set(
                            &BlendMockDataKey::Supplied(request.address.clone()),
                            &(total_supplied - actual_withdraw),
                        );
                    }
                }
                _ => panic!("unsupported request type in submit"),
            }
        }

        pub fn redeem(env: Env, asset: Address, amount: i128, to: Address) -> i128 {
            let token_client = TestTokenClient::new(&env, &asset);
            let pool_balance = token_client.balance(&env.current_contract_address());

            // Mock partial redemption: return only half if requested > 0
            let actual_to_return = if amount > 0 {
                core::cmp::min(amount, pool_balance / 2)
            } else {
                0
            };

            if actual_to_return > 0 {
                token_client.transfer(&env.current_contract_address(), &to, &actual_to_return);
            }
            actual_to_return
        }

        /// Real Blend has no `balance(asset, user)` function at all — reading
        /// a position requires `get_reserve(asset)` (for the b-rate/index)
        /// plus `get_positions(user)` (for the b-token amount), per #986's
        /// research against blend-capital/blend-contracts-v2. This mock
        /// mirrors that shape; see `blend_pool_balance` below for the
        /// combined read helper tests should call instead of a direct
        /// `balance`.
        pub fn get_reserve(_env: Env, asset: Address) -> crate::BlendReserve {
            crate::BlendReserve {
                asset,
                config: crate::BlendReserveConfig {
                    index: 0,
                    decimals: 7,
                    c_factor: 0,
                    l_factor: 0,
                    util: 0,
                    max_util: 0,
                    r_base: 0,
                    r_one: 0,
                    r_two: 0,
                    r_three: 0,
                    reactivity: 0,
                    supply_cap: 0,
                    enabled: true,
                },
                data: crate::BlendReserveData {
                    d_rate: crate::BLEND_RATE_SCALAR,
                    b_rate: crate::BLEND_RATE_SCALAR,
                    ir_mod: 0,
                    b_supply: 0,
                    d_supply: 0,
                    backstop_credit: 0,
                    last_time: 0,
                },
                scalar: 10_000_000,
            }
        }

        /// Reports the mock's actual token balance as the user's full
        /// b-token "supply" position at index 0 (this mock only ever tracks
        /// one asset/one position at a time, via `TrackedAsset`). Falls
        /// back to empty maps when no asset has ever been supplied yet —
        /// `set_blend_pool` probes this at configuration time, before any
        /// real supply has happened, and must not panic.
        pub fn get_positions(env: Env, _user: Address) -> crate::BlendPositions {
            let tracked_asset: Option<Address> = env
                .storage()
                .persistent()
                .get(&BlendMockDataKey::TrackedAsset);
            let mut supply = Map::new(&env);
            if let Some(asset) = tracked_asset {
                let pool_balance =
                    TestTokenClient::new(&env, &asset).balance(&env.current_contract_address());
                supply.set(0u32, pool_balance);
            }
            crate::BlendPositions {
                liabilities: Map::new(&env),
                collateral: Map::new(&env),
                supply,
            }
        }

        pub fn supplied(env: Env, asset: Address) -> i128 {
            env.storage()
                .persistent()
                .get(&BlendMockDataKey::Supplied(asset))
                .unwrap_or(0)
        }

        pub fn get_max_supply_limit(env: Env) -> i128 {
            env.storage()
                .persistent()
                .get(&BlendMockDataKey::MaxSupplyLimit)
                .unwrap_or(0)
        }

        pub fn get_max_withdraw_limit(env: Env) -> i128 {
            env.storage()
                .persistent()
                .get(&BlendMockDataKey::MaxWithdrawLimit)
                .unwrap_or(0)
        }
    }
}

pub use blend::{MockBlendPool, MockBlendPoolClient};

pub mod dex {
    use super::*;

    /// Minimal single-asset DEX liquidity pool mock.
    ///
    /// Implements the same entrypoints the vault's `DexPoolClient` calls:
    /// `add_liquidity`, `remove_liquidity`, and `balance`. It pulls/returns the
    /// underlying USDC via `transfer_from`/`transfer` (mirroring the Blend mock)
    /// and supports configurable supply/withdraw limits to simulate slippage and
    /// partial fills for `min_out` testing.
    #[contract]
    pub struct MockDexPool;

    #[contractimpl]
    impl MockDexPool {
        pub fn add_liquidity(
            env: Env,
            from: Address,
            asset: Address,
            amount: i128,
            _min_out: i128,
        ) -> i128 {
            from.require_auth();
            assert!(amount > 0, "amount must be positive");

            let token_client = TestTokenClient::new(&env, &asset);
            let pool = env.current_contract_address();
            let allowance = token_client.allowance(&from, &pool);
            assert!(allowance >= amount, "expected allowance before pool pull");

            // Respect a configured supply limit to simulate slippage / shortfall.
            let max_supply_limit: i128 = env
                .storage()
                .persistent()
                .get(&DexMockDataKey::MaxSupplyLimit)
                .unwrap_or(0);

            let actual_amount = if max_supply_limit < 0 {
                0
            } else if max_supply_limit > 0 {
                core::cmp::min(amount, max_supply_limit)
            } else {
                amount
            };

            if actual_amount > 0 {
                token_client.transfer_from(&pool, &from, &pool, &actual_amount);

                let supplied: i128 = env
                    .storage()
                    .persistent()
                    .get(&DexMockDataKey::Supplied(asset.clone()))
                    .unwrap_or(0);
                env.storage().persistent().set(
                    &DexMockDataKey::Supplied(asset),
                    &(supplied + actual_amount),
                );
            }

            // If a lying override is set, report that instead of the actual amount.
            let override_amount: Option<i128> = env
                .storage()
                .persistent()
                .get(&DexMockDataKey::ReportedSupplyOverride);
            override_amount.unwrap_or(actual_amount)
        }

        /// Makes add_liquidity report `reported` as the return value regardless of
        /// how much was actually transferred. Used to test that the vault measures
        /// outcome via balance-delta rather than trusting the pool's return value.
        pub fn set_reported_supply_amount(env: Env, reported: i128) {
            env.storage()
                .persistent()
                .set(&DexMockDataKey::ReportedSupplyOverride, &reported);
        }

        pub fn remove_liquidity(
            env: Env,
            to: Address,
            asset: Address,
            amount: i128,
            _min_out: i128,
        ) -> i128 {
            to.require_auth();
            assert!(amount > 0, "amount must be positive");

            let supplied: i128 = env
                .storage()
                .persistent()
                .get(&DexMockDataKey::Supplied(asset.clone()))
                .unwrap_or(0);
            let amount_to_withdraw = core::cmp::min(amount, supplied);

            // Respect a configured withdraw limit to simulate stuck liquidity.
            let max_withdraw_limit: i128 = env
                .storage()
                .persistent()
                .get(&DexMockDataKey::MaxWithdrawLimit)
                .unwrap_or(0);

            let actual_withdraw = if max_withdraw_limit > 0 {
                core::cmp::min(amount_to_withdraw, max_withdraw_limit)
            } else {
                amount_to_withdraw
            };

            if actual_withdraw > 0 {
                let token_client = TestTokenClient::new(&env, &asset);
                token_client.transfer(&env.current_contract_address(), &to, &actual_withdraw);

                env.storage().persistent().set(
                    &DexMockDataKey::Supplied(asset),
                    &(supplied - actual_withdraw),
                );
            }

            actual_withdraw
        }

        /// Returns the liquidity position held for `_user` in `asset`.
        pub fn balance(env: Env, asset: Address, _user: Address) -> i128 {
            let supplied: i128 = env
                .storage()
                .persistent()
                .get(&DexMockDataKey::Supplied(asset.clone()))
                .unwrap_or(0);
            let token_bal =
                TestTokenClient::new(&env, &asset).balance(&env.current_contract_address());
            core::cmp::max(supplied, token_bal)
        }

        /// Sets a max supply limit to simulate slippage / partial fills.
        /// 0 = no limit (default behavior); negative = reject all supply.
        pub fn set_max_supply_limit(env: Env, limit: i128) {
            env.storage()
                .persistent()
                .set(&DexMockDataKey::MaxSupplyLimit, &limit);
        }

        /// Sets a max withdraw limit to simulate stuck liquidity.
        /// 0 = no limit (default behavior).
        pub fn set_max_withdraw_limit(env: Env, limit: i128) {
            env.storage()
                .persistent()
                .set(&DexMockDataKey::MaxWithdrawLimit, &limit);
        }
    }
}

pub use dex::{MockDexPool, MockDexPoolClient};

// ============================================================================
// TEST SETUP FUNCTIONS
// ============================================================================

pub fn setup_vault(env: &Env) -> (Address, Address, Address) {
    let (contract_id, agent, owner, _usdc_token) = setup_vault_with_token(env);
    (contract_id, agent, owner)
}

/// Sets up a vault with a real deployed TestToken contract.
pub fn setup_vault_with_token(env: &Env) -> (Address, Address, Address, Address) {
    let deployer = Address::generate(env);
    let salt = BytesN::from_array(env, &[0u8; 32]);
    let contract_id = env
        .deployer()
        .with_address(deployer.clone(), salt.clone())
        .deployed_address();
    env.register_contract(&contract_id, NeuroWealthVault);

    let client = NeuroWealthVaultClient::new(env, &contract_id);
    let agent = Address::generate(env);
    let usdc_token = env.register_contract(None, TestToken);
    // Generate a distinct address for owner to decouple roles
    let owner = Address::generate(env);

    client.initialize(&deployer, &owner, &agent, &usdc_token, &salt);

    (contract_id, agent, owner, usdc_token)
}

pub fn setup_vault_with_token_and_blend(
    env: &Env,
) -> (Address, Address, Address, Address, Address) {
    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(env);
    let blend_pool = env.register_contract(None, MockBlendPool);

    (contract_id, agent, owner, usdc_token, blend_pool)
}

/// Sets up a vault with a real deployed TestToken and a MockDexPool.
///
/// Returns `(vault_id, agent, owner, usdc_token, dex_pool)`.
pub fn setup_vault_with_token_and_dex(env: &Env) -> (Address, Address, Address, Address, Address) {
    let (contract_id, agent, owner, usdc_token) = setup_vault_with_token(env);
    let dex_pool = env.register_contract(None, MockDexPool);

    (contract_id, agent, owner, usdc_token, dex_pool)
}

// ============================================================================
// EVENT HELPERS
// ============================================================================

/// Returns all events whose topics contain `topic`.
///
/// `env.events().all()` (requires `Events` trait in scope) yields
/// `(contract_address, topics, data)` tuples. The first element is the
/// emitting contract's address; the second is a `soroban_sdk::Vec<Val>` of
/// topic values; the third is the event data `Val`.
pub fn find_events_by_topic(
    events: Vec<(Address, Vec<Val>, Val)>,
    env: &Env,
    topic: Symbol,
) -> std::vec::Vec<(Address, Vec<Val>, Val)> {
    let mut result = std::vec::Vec::new();
    for i in 0..events.len() {
        if let Some((contract_addr, topics, data)) = events.get(i) {
            for j in 0..topics.len() {
                if let Some(t) = topics.get(j) {
                    if let Ok(s) = Symbol::try_from_val(env, &t) {
                        if s == topic {
                            result.push((contract_addr.clone(), topics.clone(), data));
                            break;
                        }
                    }
                }
            }
        }
    }
    result
}

// ============================================================================
// DEPOSIT HELPER
// ============================================================================

/// Mints `amount` test tokens for `user` and deposits them into the vault.
pub fn mint_and_deposit(
    env: &Env,
    vault_client: &NeuroWealthVaultClient,
    token_address: &Address,
    user: &Address,
    amount: i128,
) {
    let token_client = TestTokenClient::new(env, token_address);
    token_client.mint(user, &amount);
    vault_client.deposit(user, &amount);
}

/// Reads a user's underlying-asset balance in the mock Blend pool the same
/// way the real `BlendPoolClient::get_balance` does (#986): combine
/// `get_reserve` (for the b-rate/index) with `get_positions` (for the
/// b-token amount), then convert. Tests should call this instead of a
/// direct `balance` call, which no longer exists on either the real Blend
/// pool or this mock.
pub fn blend_pool_balance(
    env: &Env,
    pool_address: &Address,
    asset: &Address,
    user: &Address,
) -> i128 {
    let pool_client = blend::MockBlendPoolClient::new(env, pool_address);
    let reserve = pool_client.get_reserve(asset);
    let positions = pool_client.get_positions(user);
    let b_tokens = positions.supply.get(reserve.config.index).unwrap_or(0);
    if b_tokens == 0 {
        return 0;
    }
    b_tokens * reserve.data.b_rate / crate::BLEND_RATE_SCALAR
}

// ============================================================================
// STORAGE-DIFF HARNESS (Issue #596 — adversarial-agent simulation)
// ============================================================================

/// A snapshot of every publicly observable field of vault state, read back
/// entirely through the contract's own getter entrypoints (never by reaching
/// into `env.storage()` directly, since Soroban has no API to enumerate a
/// contract's storage keys generically).
///
/// Used by the adversarial-agent simulation to prove that a call which was
/// supposed to be rejected produced **zero** observable mutation, and that a
/// call which is legitimately permitted only touched the fields it is
/// allowed to.
#[derive(Clone, Debug, PartialEq)]
pub struct VaultStateSnapshot {
    pub owner: Address,
    pub agent: Address,
    pub paused: bool,
    pub tvl_cap: i128,
    pub user_deposit_cap: i128,
    pub min_deposit: i128,
    pub max_deposit: i128,
    pub rebalance_cooldown: u32,
    pub max_consecutive_failures: u32,
    pub consecutive_failures: u32,
    pub approval_ttl: u32,
    pub blend_pool: Option<Address>,
    pub dex_pool: Option<Address>,
    pub blend_approval_ttl: u32,
    pub pending_owner: Option<Address>,
    pub pending_agent_update: Option<(Address, u32)>,
    pub pending_upgrade: Option<(BytesN<32>, u32)>,
    pub total_assets: i128,
    pub total_shares: i128,
    pub current_protocol: Symbol,
    pub last_rebalance_ledger: u32,
    pub version: u32,
    /// `(address, get_balance, get_shares, get_user_strategy)` for each
    /// address the caller asked to watch. Kept separate from the top-level
    /// fields above because it is per-address rather than singleton state.
    pub watched: std::vec::Vec<(Address, i128, i128, Symbol)>,
}

/// Reads back the full observable state of `client`'s vault, including
/// per-address data for every address in `watch`.
pub fn snapshot_vault_state(
    client: &NeuroWealthVaultClient,
    watch: &[Address],
) -> VaultStateSnapshot {
    VaultStateSnapshot {
        owner: client.get_owner(),
        agent: client.get_agent(),
        paused: client.is_paused(),
        tvl_cap: client.get_tvl_cap(),
        user_deposit_cap: client.get_user_deposit_cap(),
        min_deposit: client.get_min_deposit(),
        max_deposit: client.get_max_deposit(),
        rebalance_cooldown: client.get_rebalance_cooldown(),
        max_consecutive_failures: client.get_max_consecutive_failures(),
        consecutive_failures: client.get_consecutive_failures(),
        approval_ttl: client.get_approval_ttl(),
        blend_pool: client.get_blend_pool(),
        dex_pool: client.get_dex_pool(),
        blend_approval_ttl: client.get_blend_approval_ttl(),
        pending_owner: client.get_pending_owner(),
        pending_agent_update: client.get_pending_agent_update(),
        pending_upgrade: client.get_pending_upgrade(),
        total_assets: client.get_total_assets(),
        total_shares: client.get_total_shares(),
        current_protocol: client.get_current_protocol(),
        last_rebalance_ledger: client.get_last_rebalance_ledger(),
        version: client.get_version(),
        watched: watch
            .iter()
            .map(|a| {
                (
                    a.clone(),
                    client.get_balance(a),
                    client.get_shares(a),
                    client.get_user_strategy(a),
                )
            })
            .collect(),
    }
}

/// Returns the names of every top-level (singleton) field that differs
/// between two snapshots. Per-address data is reported separately by
/// [`diff_watched_addresses`] since it is keyed rather than singleton.
pub fn diff_vault_state(
    before: &VaultStateSnapshot,
    after: &VaultStateSnapshot,
) -> std::vec::Vec<&'static str> {
    let mut changed = std::vec::Vec::new();
    macro_rules! check {
        ($field:ident) => {
            if before.$field != after.$field {
                changed.push(stringify!($field));
            }
        };
    }
    check!(owner);
    check!(agent);
    check!(paused);
    check!(tvl_cap);
    check!(user_deposit_cap);
    check!(min_deposit);
    check!(max_deposit);
    check!(rebalance_cooldown);
    check!(max_consecutive_failures);
    check!(consecutive_failures);
    check!(approval_ttl);
    check!(blend_pool);
    check!(dex_pool);
    check!(blend_approval_ttl);
    check!(pending_owner);
    check!(pending_agent_update);
    check!(pending_upgrade);
    check!(total_assets);
    check!(total_shares);
    check!(current_protocol);
    check!(last_rebalance_ledger);
    check!(version);
    changed
}

/// Returns the watched addresses whose `(balance, shares, strategy)` tuple
/// changed between two snapshots. `before` and `after` must have been taken
/// with the same `watch` list, in the same order.
pub fn diff_watched_addresses(
    before: &VaultStateSnapshot,
    after: &VaultStateSnapshot,
) -> std::vec::Vec<Address> {
    before
        .watched
        .iter()
        .zip(after.watched.iter())
        .filter(|(b, a)| b.1 != a.1 || b.2 != a.2 || b.3 != a.3)
        .map(|(b, _)| b.0.clone())
        .collect()
}

/// Scopes Soroban authorization to *only* `signer`'s signature for exactly
/// one contract invocation (`fn_name` + `args` on `contract_id`), with no
/// sub-invocations authorized either.
///
/// This is the tool that makes the "compromised agent" simulation honest: a
/// real compromised agent controls only its own signing key, never the
/// owner's. `env.mock_all_auths()` cannot express that distinction because it
/// authorizes every address unconditionally, so entrypoints that fetch their
/// authorizing address from storage (`require_is_owner`, `require_is_agent`)
/// would appear to accept any caller. Scoping the mock to exactly the agent's
/// address, with no owner entry present, makes those entrypoints fail auth
/// resolution exactly as they would on a real network.
pub fn mock_agent_only_auth(
    env: &Env,
    contract_id: &Address,
    agent: &Address,
    fn_name: &'static str,
    args: Vec<Val>,
) {
    env.mock_auths(&[soroban_sdk::testutils::MockAuth {
        address: agent,
        invoke: &soroban_sdk::testutils::MockAuthInvoke {
            contract: contract_id,
            fn_name,
            args,
            sub_invokes: &[],
        },
    }]);
}
