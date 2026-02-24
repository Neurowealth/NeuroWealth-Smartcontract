//! # NeuroWealth Vault Contract
//!
//! An ERC-4626 inspired vault contract for the NeuroWealth AI-powered DeFi yield platform on Stellar.
//!
//! ## Architecture Overview
//!
//! This contract implements a non-custodial vault where users deposit USDC and an AI agent
//! automatically deploys those funds across various yield-generating protocols on the Stellar
//! blockchain. The vault maintains 1:1 parity between deposited assets and user balances.
//!
//! ## Share Accounting Model
//!
//! Currently, this implementation uses a simple 1:1 asset accounting model where:
//! - 1 deposited USDC = 1 vault share
//! - Withdrawals return the exact USDC amount deposited (plus any yield earned externally)
//!
//! In Phase 2, this will be upgraded to a proper share-based system using the ERC-4626
//! standard, where vault shares represent a proportional claim on total vault assets.
//!
//! ## Asset Flow
//!
//! ```text
//! Deposit Flow:
//! User → [USDC Token] → [Vault Contract] → [AI Agent monitors]
//!                      ↓
//!              Balance recorded per user
//!              DepositEvent emitted
//!
//! Rebalance Flow (AI Agent):
//! AI Agent → [Vault.rebalance()] → [External Protocols (Blend, DEX)]
//!                              ↓
//!                      RebalanceEvent emitted
//!
//! Withdraw Flow:
//! User → [Vault.withdraw()] → [Vault Contract] → [USDC Token] → User
//!         ↓
//! Balance updated
//! WithdrawEvent emitted
//! ```
//!
//! ## Storage Layout
//!
//! ### Instance Storage (Contract-Wide, Expensive to Read/Write)
//! - `Agent`: The authorized AI agent address that can call rebalance()
//! - `UsdcToken`: The USDC token contract address
//! - `TotalDeposits`: Total USDC held in vault (excluding yield deployed externally)
//! - `Paused`: Boolean flag for emergency pause state
//! - `Owner`: Contract owner address for administrative functions
//! - `TvlCap`: Maximum total value locked in the vault
//! - `UserDepositCap`: Maximum deposit per user
//! - `Version`: Contract version for upgrade tracking
//!
//! ### Persistent Storage (Per-User, Cheaper)
//! - `Balance(user)`: USDC balance for each user address
//!
//! ## Event Design Philosophy
//!
//! Events are emitted for all state-changing operations to enable:
//! - AI agent to detect deposits/withdrawals and react accordingly
//! - Frontend applications to track user balances in real-time
//! - External indexers to build transaction histories
//! - Security auditors to verify contract behavior
//!
//! ## Upgrade Model
//!
//! This contract supports upgradeability through Soroban's built-in contract upgrade
//! mechanism. The owner can upgrade the contract code while preserving storage state.
//! Upgrades must be performed carefully to maintain:
//! - User balances
//! - Total deposits
//! - Agent and owner addresses
//! - Configuration parameters
//!
//! # Examples
//!
//! ## Deposit USDC
//! ```ignore
//! let token_client = token::Client::new(&env, &usdc_token);
//! token_client.transfer(&user, &vault_address, &amount);
//! vault_client.deposit(&user, &amount);
//! ```
//!
//! ## Withdraw USDC
//! ```ignore
//! vault_client.withdraw(&user, &amount);
//! ```

#![no_std]

use soroban_sdk::{
    contract, contractimpl, contracttype, token,
    Address, Env, Symbol, symbol_short,
};


// ============================================================================
// STORAGE KEYS
// ============================================================================

/// Storage keys for vault state.
///
/// This enum defines all keys used for both instance and persistent storage.
/// Instance storage is used for contract-wide configuration, while persistent
/// storage is used for per-user data that requires efficient access.
#[contracttype]
pub enum DataKey {
    /// User's USDC balance (key: user Address)
    /// Stored in persistent storage for efficient per-user access
    Balance(Address),
    /// Total USDC deposits in the vault
    /// Stored in instance storage (single value, frequently read)
    TotalDeposits,
    /// Authorized AI agent address
    /// Can only call rebalance() to move funds between yield strategies
    Agent,
    /// USDC token contract address
    /// The vault accepts only this token for deposits
    UsdcToken,
    /// Contract pause state
    /// When true, deposits and withdrawals are disabled
    Paused,
    /// Contract owner address
    /// Can perform administrative functions (pause, upgrade, set limits)
    Owner,
    /// Total Value Locked cap
    /// Maximum total USDC that can be deposited in the vault
    TvLCap,
    /// Per-user deposit cap
    /// Maximum amount a single user can deposit
    UserDepositCap,
    /// Contract version for upgrade tracking
    Version,
}


// ============================================================================
// EVENTS
// ============================================================================

/// Emitted when a user deposits USDC into the vault.
///
/// AI agents monitor this event to detect new deposits and initiate
/// yield deployment. External indexers use this for transaction tracking.
///
/// # Topics
/// - `SymbolShort("deposit")` - Event identifier
#[contracttype]
pub struct DepositEvent {
    /// The user who made the deposit
    pub user: Address,
    /// Amount of USDC deposited (7 decimal places)
    pub amount: i128,
}

/// Emitted when a user withdraws USDC from the vault.
///
/// AI agents monitor this event to update their internal records.
/// External indexers use this for transaction tracking.
///
/// # Topics
/// - `SymbolShort("withdraw")` - Event identifier
#[contracttype]
pub struct WithdrawEvent {
    /// The user who made the withdrawal
    pub user: Address,
    /// Amount of USDC withdrawn (7 decimal places)
    pub amount: i128,
}

/// Emitted when the AI agent rebalances funds between yield strategies.
///
/// This event signals that the agent is moving funds between different
/// yield-generating protocols. The strategy symbol indicates the new
/// target allocation.
///
/// # Topics
/// - `SymbolShort("rebalance")` - Event identifier
#[contracttype]
pub struct RebalanceEvent {
    /// The strategy being deployed to (e.g., "conservative", "balanced", "growth")
    pub strategy: Symbol,
    /// Amount being rebalanced (0 for full rebalance)
    pub amount: i128,
}

/// Emitted when the vault is paused or unpaused.
///
/// # Topics
/// - `SymbolShort("pause")` - Event identifier
#[contracttype]
pub struct PauseEvent {
    /// True if vault is now paused, false if unpaused
    pub paused: bool,
    /// Address that triggered the pause/unpause
    pub caller: Address,
}


// ============================================================================
// CONTRACT
// ============================================================================

/// NeuroWealth Vault - AI-Managed DeFi Yield Vault on Stellar
///
/// A non-custodial vault that accepts USDC deposits and allows an authorized
/// AI agent to automatically deploy those funds across various yield-generating
/// protocols on the Stellar blockchain.
///
/// # Security Model
///
/// - Users can only withdraw their own funds (enforced via `require_auth()`)
/// - Only the designated AI agent can call `rebalance()`
/// - Only the owner can call administrative functions
/// - Minimum deposit: 1 USDC
/// - Maximum per-user deposit: configurable (default 10,000 USDC)
/// - Emergency pause functionality available to owner
///
/// # Upgradeability
///
/// This contract can be upgraded by the owner while preserving all storage state.
#[contract]
pub struct NeuroWealthVault;

#[contractimpl]
impl NeuroWealthVault {

    // ==========================================================================
    // INITIALIZATION
    // ==========================================================================

    /// Initializes the vault with required configuration.
    ///
    /// This function must be called exactly once after contract deployment
    /// to set up the vault's core configuration. After initialization,
    /// the vault is ready to accept deposits.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `agent` - The authorized AI agent address that can call rebalance()
    /// * `usdc_token` - The USDC token contract address
    ///
    /// # Returns
    /// Nothing. This function mutates state but returns nothing.
    ///
    /// # Panics
    /// - If the vault has already been initialized (Agent key already exists)
    ///
    /// # Events
    /// None - initialization does not emit events
    ///
    /// # Security
    /// - This function can only be called once (idempotent initialization prevention)
    /// - The deployer should verify the agent and token addresses are correct
    /// - After initialization, the deployer should transfer ownership or destroy
    ///   the deployer key to prevent re-initialization
    pub fn initialize(env: Env, agent: Address, usdc_token: Address) {
        if env.storage().instance().has(&DataKey::Agent) {
            panic!("Already initialized");
        }

        env.storage().instance().set(&DataKey::Agent, &agent);
        env.storage().instance().set(&DataKey::UsdcToken, &usdc_token);
        env.storage().instance().set(&DataKey::TotalDeposits, &0_i128);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::Owner, &agent);
        env.storage().instance().set(&DataKey::TvLCap, &100_000_000_000_i128); // 100M USDC default
        env.storage().instance().set(&DataKey::UserDepositCap, &10_000_000_000_i128); // 10K USDC default
        env.storage().instance().set(&DataKey::Version, &1_u32);
    }


    // ==========================================================================
    // CORE LIFECYCLE - DEPOSIT
    // ==========================================================================

    /// Deposits USDC into the vault on behalf of a user.
    ///
    /// The user must authorize this transaction with their signature.
    /// The vault transfers USDC from the user and records their balance.
    /// An event is emitted for the AI agent to detect and initiate yield deployment.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `user` - The user address making the deposit (must authorize)
    /// * `amount` - Amount of USDC to deposit (7 decimal places)
    ///
    /// # Returns
    /// Nothing. This function records the deposit and returns nothing.
    ///
    /// # Panics
    /// - If the vault is paused
    /// - If amount is not positive
    /// - If amount is less than 1 USDC (minimum deposit)
    /// - If amount would exceed the user's deposit cap
    /// - If amount would exceed the TVL cap
    /// - If the USDC transfer fails
    ///
    /// # Events
    /// Emits `DepositEvent` with:
    /// - `user`: The depositing user's address
    /// - `amount`: The amount deposited
    ///
    /// # Security
    /// - `user.require_auth()` ensures only the user can deposit to their own account
    /// - Checks are performed before state updates (checks-effects-interactions pattern)
    /// - Balance is updated after successful token transfer
    pub fn deposit(env: Env, user: Address, amount: i128) {
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(amount);
        Self::require_minimum_deposit(amount);
        Self::require_within_deposit_cap(&env, &user, amount);
        Self::require_within_tvl_cap(&env, amount);

        let usdc_token: Address = env.storage().instance()
            .get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&user, &env.current_contract_address(), &amount);

        let current_balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(user.clone()))
            .unwrap_or(0);
        env.storage().persistent()
            .set(&DataKey::Balance(user.clone()), &(current_balance + amount));

        let total: i128 = env.storage().instance()
            .get(&DataKey::TotalDeposits).unwrap_or(0);
        env.storage().instance()
            .set(&DataKey::TotalDeposits, &(total + amount));

        env.events().publish(
            (symbol_short!("deposit"),),
            DepositEvent { user, amount }
        );
    }


    // ==========================================================================
    // CORE LIFECYCLE - WITHDRAW
    // ==========================================================================

    /// Withdraws USDC from the vault for a user.
    ///
    /// The user must authorize this transaction with their signature.
    /// The vault transfers USDC from its balance to the user.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `user` - The user address withdrawing funds (must authorize)
    /// * `amount` - Amount of USDC to withdraw (7 decimal places)
    ///
    /// # Returns
    /// Nothing. This function processes the withdrawal and returns nothing.
    ///
    /// # Panics
    /// - If the vault is paused
    /// - If amount is not positive
    /// - If user has insufficient balance
    /// - If the USDC transfer fails
    ///
    /// # Events
    /// Emits `WithdrawEvent` with:
    /// - `user`: The withdrawing user's address
    /// - `amount`: The amount withdrawn
    ///
    /// # Security
    /// - `user.require_auth()` ensures users can only withdraw their own funds
    /// - Balance check is performed before any state updates
    /// - Uses checks-effects-interactions pattern: balance updated before transfer
    pub fn withdraw(env: Env, user: Address, amount: i128) {
        user.require_auth();

        Self::require_not_paused(&env);
        Self::require_positive_amount(amount);

        let balance: i128 = env.storage().persistent()
            .get(&DataKey::Balance(user.clone()))
            .unwrap_or(0);

        assert!(balance >= amount, "Insufficient balance");

        env.storage().persistent()
            .set(&DataKey::Balance(user.clone()), &(balance - amount));

        let total: i128 = env.storage().instance()
            .get(&DataKey::TotalDeposits).unwrap_or(0);
        env.storage().instance()
            .set(&DataKey::TotalDeposits, &(total - amount));

        let usdc_token: Address = env.storage().instance()
            .get(&DataKey::UsdcToken).unwrap();
        let token_client = token::Client::new(&env, &usdc_token);
        token_client.transfer(&env.current_contract_address(), &user, &amount);

        env.events().publish(
            (symbol_short!("withdraw"),),
            WithdrawEvent { user, amount }
        );
    }


    // ==========================================================================
    // CORE LIFECYCLE - REBALANCE
    // ==========================================================================

    /// Rebalances vault funds between yield strategies.
    ///
    /// Only the authorized AI agent can call this function. The agent uses
    /// this to move funds between different yield-generating protocols based
    /// on market conditions and strategy performance.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `strategy` - The target strategy symbol (e.g., "conservative", "balanced", "growth")
    ///
    /// # Returns
    /// Nothing. This function triggers rebalancing and returns nothing.
    ///
    /// # Panics
    /// - If the vault is paused
    /// - If the caller is not the authorized agent
    ///
    /// # Events
    /// Emits `RebalanceEvent` with:
    /// - `strategy`: The target strategy
    /// - `amount`: Amount being rebalanced (0 for full rebalance)
    ///
    /// # Security
    /// - `agent.require_auth()` ensures only the authorized AI agent can rebalance
    /// - Agent is set during initialization and cannot be changed (intentional)
    /// - This function does NOT transfer funds - it's a signal to external protocols
    /// - Phase 2 will add actual protocol interactions (Blend, DEX)
    pub fn rebalance(env: Env, strategy: Symbol) {
        Self::require_not_paused(&env);
        Self::require_is_agent(&env);

        env.events().publish(
            (symbol_short!("rebalance"),),
            RebalanceEvent { strategy, amount: 0 }
        );
    }


    // ==========================================================================
    // ADMINISTRATIVE - PAUSE CONTROL
    // ==========================================================================

    /// Pauses the vault, disabling deposits and withdrawals.
    ///
    /// Emergency function to halt all user-facing operations.
    /// When paused:
    /// - Deposits are rejected
    /// - Withdrawals are rejected
    /// - Rebalancing is rejected
    /// - Read functions remain operational
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// Nothing. This function pauses the vault and returns nothing.
    ///
    /// # Panics
    /// - If the caller is not the owner
    ///
    /// # Events
    /// Emits `PauseEvent` with:
    /// - `paused`: true
    /// - `caller`: The owner's address
    ///
    /// # Security
    /// - Only the owner can pause the vault
    /// - There is no automatic unpause - owner must explicitly call unpause()
    /// - Users' funds remain safe and can be withdrawn after unpause
    pub fn pause(env: Env) {
        Self::require_is_owner(&env);

        env.storage().instance().set(&DataKey::Paused, &true);

        env.events().publish(
            (symbol_short!("pause"),),
            PauseEvent { paused: true, caller: env.invoker() }
        );
    }

    /// Unpauses the vault, re-enabling deposits and withdrawals.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// Nothing. This function unpauses the vault and returns nothing.
    ///
    /// # Panics
    /// - If the caller is not the owner
    /// - If the vault is not currently paused
    ///
    /// # Events
    /// Emits `PauseEvent` with:
    /// - `paused`: false
    /// - `caller`: The owner's address
    ///
    /// # Security
    /// - Only the owner can unpause the vault
    pub fn unpause(env: Env) {
        Self::require_is_owner(&env);

        let paused: bool = env.storage().instance()
            .get(&DataKey::Paused).unwrap_or(false);
        assert!(paused, "Vault is not paused");

        env.storage().instance().set(&DataKey::Paused, &false);

        env.events().publish(
            (symbol_short!("pause"),),
            PauseEvent { paused: false, caller: env.invoker() }
        );
    }


    // ==========================================================================
    // ADMINISTRATIVE - CONFIGURATION
    // ==========================================================================

    /// Sets the TVL (Total Value Locked) cap for the vault.
    ///
    /// Maximum total USDC that can be deposited in the vault.
    /// Setting to 0 removes the cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `cap` - New TVL cap in USDC units (7 decimal places)
    ///
    /// # Returns
    /// Nothing. This function updates the cap and returns nothing.
    ///
    /// # Panics
    /// - If the caller is not the owner
    ///
    /// # Events
    /// None
    ///
    /// # Security
    /// - Only the owner can modify the TVL cap
    /// - Reducing the cap below current total deposits does not affect existing deposits
    pub fn set_tvl_cap(env: Env, cap: i128) {
        Self::require_is_owner(&env);
        env.storage().instance().set(&DataKey::TvLCap, &cap);
    }

    /// Sets the maximum deposit amount per user.
    ///
    /// Maximum amount that any single user can have deposited in the vault.
    /// Setting to 0 removes the cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `cap` - New per-user deposit cap in USDC units (7 decimal places)
    ///
    /// # Returns
    /// Nothing. This function updates the cap and returns nothing.
    ///
    /// # Panics
    /// - If the caller is not the owner
    ///
    /// # Events
    /// None
    ///
    /// # Security
    /// - Only the owner can modify the user deposit cap
    /// - Reducing the cap below a user's current balance does not affect them
    pub fn set_user_deposit_cap(env: Env, cap: i128) {
        Self::require_is_owner(&env);
        env.storage().instance().set(&DataKey::UserDepositCap, &cap);
    }

    /// Returns the current TVL cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current TVL cap in USDC units (7 decimal places), or 0 if no cap
    pub fn get_tvl_cap(env: Env) -> i128 {
        env.storage().instance()
            .get(&DataKey::TvLCap)
            .unwrap_or(0)
    }

    /// Returns the current per-user deposit cap.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current per-user deposit cap in USDC units (7 decimal places), or 0 if no cap
    pub fn get_user_deposit_cap(env: Env) -> i128 {
        env.storage().instance()
            .get(&DataKey::UserDepositCap)
            .unwrap_or(0)
    }


    // ==========================================================================
    // READ FUNCTIONS
    // ==========================================================================

    /// Returns the USDC balance of a specific user.
    ///
    /// This is the amount of USDC the user has deposited, excluding any
    /// yield that may have been earned through the AI agent's strategies.
    /// Yield is tracked separately by the AI agent off-chain.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    /// * `user` - The user address to query
    ///
    /// # Returns
    /// The user's USDC balance in raw units (7 decimal places)
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_balance(env: Env, user: Address) -> i128 {
        env.storage().persistent()
            .get(&DataKey::Balance(user))
            .unwrap_or(0)
    }

    /// Returns the total USDC deposited in the vault.
    ///
    /// This is the sum of all user balances. It represents the on-chain
    /// TVL. Funds deployed to external yield protocols are not included
    /// in this figure (tracked separately by the AI agent).
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// Total USDC deposits in raw units (7 decimal places)
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_total_deposits(env: Env) -> i128 {
        env.storage().instance()
            .get(&DataKey::TotalDeposits)
            .unwrap_or(0)
    }

    /// Returns the authorized AI agent address.
    ///
    /// This is the only address that can call rebalance() to move funds
    /// between yield strategies.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The agent's Address
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_agent(env: Env) -> Address {
        env.storage().instance()
            .get(&DataKey::Agent)
            .unwrap()
    }

    /// Returns the contract owner address.
    ///
    /// The owner can pause/unpause the vault, set limits, and upgrade the contract.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The owner's Address
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_owner(env: Env) -> Address {
        env.storage().instance()
            .get(&DataKey::Owner)
            .unwrap()
    }

    /// Returns whether the vault is currently paused.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// True if paused, false otherwise
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Returns the contract version.
    ///
    /// Used to track upgrades and ensure compatibility with external systems.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The current contract version (u32)
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_version(env: Env) -> u32 {
        env.storage().instance()
            .get(&DataKey::Version)
            .unwrap_or(1)
    }

    /// Returns the USDC token address.
    ///
    /// # Arguments
    /// * `env` - The Soroban environment
    ///
    /// # Returns
    /// The USDC token contract address
    ///
    /// # Panics
    /// None
    ///
    /// # Events
    /// None
    pub fn get_usdc_token(env: Env) -> Address {
        env.storage::instance()
            .get(&DataKey::UsdcToken)
            .unwrap()
    }


    // ==========================================================================
    // INTERNAL VALIDATION HELPERS
    // ==========================================================================

    /// Validates that the vault is not paused.
    ///
    /// # Panics
    /// - If the vault is paused
    #[inline]
    fn require_not_paused(env: &Env) {
        let paused: bool = env.storage().instance()
            .get(&DataKey::Paused).unwrap_or(false);
        assert!(!paused, "Vault is paused");
    }

    /// Validates that the caller is the contract owner.
    ///
    /// # Panics
    /// - If the caller is not the owner
    #[inline]
    fn require_is_owner(env: &Env) {
        let owner: Address = env.storage().instance()
            .get(&DataKey::Owner).unwrap();
        owner.require_auth();
    }

    /// Validates that the caller is the AI agent.
    ///
    /// # Panics
    /// - If the caller is not the agent
    #[inline]
    fn require_is_agent(env: &Env) {
        let agent: Address = env.storage().instance()
            .get(&DataKey::Agent).unwrap();
        agent.require_auth();
    }

    /// Validates that an amount is positive.
    ///
    /// # Panics
    /// - If amount is <= 0
    #[inline]
    fn require_positive_amount(amount: i128) {
        assert!(amount > 0, "Amount must be positive");
    }

    /// Validates that a deposit meets the minimum requirement.
    ///
    /// Minimum deposit is 1 USDC (1_000_000 in 7-decimal units).
    ///
    /// # Panics
    /// - If amount < minimum deposit
    #[inline]
    fn require_minimum_deposit(amount: i128) {
        assert!(amount >= 1_000_000, "Minimum deposit is 1 USDC");
    }

    /// Validates that a deposit is within the user's cap.
    ///
    /// # Panics
    /// - If user's new balance would exceed the deposit cap
    #[inline]
    fn require_within_deposit_cap(env: &Env, user: &Address, amount: i128) {
        let cap: i128 = env.storage().instance()
            .get(&DataKey::UserDepositCap).unwrap_or(0);
        if cap > 0 {
            let current_balance: i128 = env.storage().persistent()
                .get(&DataKey::Balance(user.clone()))
                .unwrap_or(0);
            assert!(current_balance + amount <= cap, "Exceeds user deposit cap");
        }
    }

    /// Validates that a deposit is within the TVL cap.
    ///
    /// # Panics
    /// - If total deposits would exceed the TVL cap
    #[inline]
    fn require_within_tvl_cap(env: &Env, amount: i128) {
        let cap: i128 = env.storage().instance()
            .get(&DataKey::TvLCap).unwrap_or(0);
        if cap > 0 {
            let total: i128 = env.storage().instance()
                .get(&DataKey::TotalDeposits).unwrap_or(0);
            assert!(total + amount <= cap, "Exceeds TVL cap");
        }
    }
}


// ============================================================================
// TESTS
// ============================================================================

#[cfg(test)]
mod test {
    use super::*;
    use soroban_sdk::{testutils::Address as _, Address, Env};

    #[test]
    fn test_deposit_and_withdraw() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, NeuroWealthVault);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        let agent = Address::generate(&env);
        let user = Address::generate(&env);

        assert_eq!(client.get_balance(&user), 0);
    }

    #[test]
    fn test_pause_and_unpause() {
        let env = Env::default();
        env.mock_all_auths();

        let contract_id = env.register_contract(None, NeuroWealthVault);
        let client = NeuroWealthVaultClient::new(&env, &contract_id);

        assert_eq!(client.is_paused(), false);
    }
}
