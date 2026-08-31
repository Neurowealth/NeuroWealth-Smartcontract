//! LibFuzzer harness: random deposit / withdraw / harvest / rebalance sequences.
//!
//! Extends `deposit_withdraw_sequence` to interleave `harvest()` and `rebalance()`
//! calls, ensuring the harvest code paths handle arbitrary inputs without panics
//! or state corruption.
//!
//! # Allowed panics (documented vault validation)
//! - `Error(Contract, #37)` — AmountMustBePositive
//! - `Error(Contract, #38)` — BelowMinimumDeposit
//! - `Error(Contract, #39)` — MaximumDepositExceeded
//! - `Error(Contract, #40)` — ExceedsUserDepositCap
//! - `Error(Contract, #41)` — ExceedsTvlCap
//! - `Error(Contract, #7)`  — InsufficientLiquidity
//! - `Error(Contract, #6)`  — SharesToMintMustBePositive
//! - `Error(Contract, #17)` — UnsupportedProtocol (harvest on "none" protocol)
//! - `Error(Contract, #43)` — RebalanceCooldownActive
//! - `Error(Contract, #42)` — MinOutNotMet
//! - Token transfer failures (`insufficient balance`, etc.)
//!
//! # Input format
//! 4-byte chunks:
//! - Byte 0: operation selector (0=deposit, 1=withdraw, 2=harvest, 3=rebalance)
//! - Bytes 1-2: amount / min_out selector (u16 LE, scaled to stroop range)
//! - Byte 3: target protocol index for rebalance (0=none, 1=blend, 2=dex)
//!
//! # Invariants checked after every successful operation
//! - `user_shares >= 0`
//! - `user_balance >= 0`
//! - `user_shares <= total_shares`
//! - `total_assets >= 0`
//! - If `total_shares > 0`: `user_balance <= total_assets`
//! - If `total_shares == 0`: `user_balance == 0`
//! - `TotalAssets` does not decrease after a harvest (yield can only be non-negative)

#![no_main]

use libfuzzer_sys::fuzz_target;
use neurowealth_vault::{NeuroWealthVault, NeuroWealthVaultClient};
use soroban_sdk::testutils::Address as _;
use soroban_sdk::{symbol_short, Address, BytesN, Env, Symbol};

// ── Minimal mock token (identical to deposit_withdraw_sequence) ──────────────

mod token {
    use soroban_sdk::{contract, contractimpl, contracttype, Address, Env};

    #[contracttype]
    enum TokenDataKey {
        Balance(Address),
    }

    #[contract]
    pub struct FuzzToken;

    #[contractimpl]
    impl FuzzToken {
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
    }
}

use token::{FuzzToken, FuzzTokenClient};

const MIN_DEPOSIT: i128 = 1_000_000;
const MAX_DEPOSIT: i128 = 100_000_000;
const TVL_CAP: i128 = 1_000_000_000_000;
const USER_CAP: i128 = 500_000_000_000;
const TOKEN_FLOAT: i128 = 1_000_000_000_000_000;

fn setup(
    env: &Env,
) -> (
    NeuroWealthVaultClient<'static>,
    Address,
    Address,
    Address,
    Address,
) {
    let deployer = Address::generate(env);
    let owner = deployer.clone();
    let agent = Address::generate(env);
    let salt = BytesN::<32>::from_array(env, &[0u8; 32]);

    let contract_id = env.register_contract(None, NeuroWealthVault);
    let client = NeuroWealthVaultClient::new(env, &contract_id);

    let usdc = env.register_contract(None, FuzzToken);
    let blend_pool = env.register_contract(None, FuzzToken);
    let user = Address::generate(env);

    client.initialize(&deployer, &owner, &agent, &usdc, &salt);

    let token = FuzzTokenClient::new(env, &usdc);
    token.mint(&user, &TOKEN_FLOAT);
    // Mint to vault so harvest can "withdraw yield"
    token.mint(&contract_id, &TOKEN_FLOAT);

    (client, user, agent, blend_pool, usdc)
}

fn is_allowed_panic(msg: &str) -> bool {
    const ALLOWED: &[&str] = &[
        "Error(Contract, #37)", // AmountMustBePositive
        "Error(Contract, #38)", // BelowMinimumDeposit
        "Error(Contract, #39)", // MaximumDepositExceeded
        "Error(Contract, #40)", // ExceedsUserDepositCap
        "Error(Contract, #41)", // ExceedsTvlCap
        "Error(Contract, #7)",  // InsufficientLiquidity
        "Error(Contract, #6)",  // SharesToMintMustBePositive
        "Error(Contract, #17)", // UnsupportedProtocol (harvest when protocol="none")
        "Error(Contract, #43)", // RebalanceCooldownActive
        "Error(Contract, #42)", // MinOutNotMet
        "Error(Contract, #16)", // MinOutMustBeNonNegative
        "insufficient balance",
        "amount must be positive",
    ];
    ALLOWED.iter().any(|needle| msg.contains(needle))
}

fn assert_vault_invariants(client: &NeuroWealthVaultClient, user: &Address) {
    let total_shares = client.get_total_shares();
    let total_assets = client.get_total_assets();
    let user_shares = client.get_shares(user);
    let user_balance = client.get_balance(user);

    assert!(total_assets >= 0, "total_assets must be non-negative");
    assert!(user_shares >= 0, "user_shares must be non-negative");
    assert!(user_balance >= 0, "user_balance must be non-negative");
    assert!(
        user_shares <= total_shares,
        "user_shares ({user_shares}) must not exceed total_shares ({total_shares})"
    );
    if total_shares > 0 {
        assert!(
            user_balance <= total_assets,
            "user_balance ({user_balance}) must not exceed total_assets ({total_assets})"
        );
    } else {
        assert_eq!(
            user_balance, 0,
            "user_balance must be 0 when no shares exist"
        );
    }
}

fuzz_target!(|data: &[u8]| {
    if data.is_empty() {
        return;
    }

    let env = Env::default();
    env.mock_all_auths();

    let (client, user, agent, _blend_pool, usdc) = setup(&env);
    let token = FuzzTokenClient::new(&env, &usdc);

    // Protocol symbols available for rebalance
    let protocols: [Symbol; 3] = [
        symbol_short!("none"),
        symbol_short!("blend"),
        symbol_short!("dex"),
    ];

    let mut assets_before_harvest: i128 = 0;

    for (i, chunk) in data.chunks(4).enumerate() {
        if chunk.is_empty() {
            continue;
        }
        let op = chunk[0] % 4;
        let raw = u16::from(chunk.get(1).copied().unwrap_or(0))
            | (u16::from(chunk.get(2).copied().unwrap_or(0)) << 8);
        let amount = i128::from(raw % 20_000) * MIN_DEPOSIT + MIN_DEPOSIT;
        let min_out: i128 = 0; // conservative — let harvest proceed with zero slippage guard
        let proto_idx = (chunk.get(3).copied().unwrap_or(0) % 3) as usize;

        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match op {
            // ── deposit ────────────────────────────────────────────────────
            0 => {
                if !(MIN_DEPOSIT..=MAX_DEPOSIT).contains(&amount)
                    || amount > USER_CAP
                    || amount > TVL_CAP
                {
                    return;
                }
                if token.balance(&user) < amount {
                    return;
                }
                client.deposit(&user, &amount);
            }
            // ── withdraw ───────────────────────────────────────────────────
            1 => {
                let balance = client.get_balance(&user);
                if balance <= 0 {
                    return;
                }
                let withdraw_amount = amount.min(balance);
                if withdraw_amount <= 0 {
                    return;
                }
                client.withdraw(&user, &withdraw_amount);
            }
            // ── harvest ────────────────────────────────────────────────────
            2 => {
                assets_before_harvest = client.get_total_assets();
                client.harvest(&min_out);
                // Yield harvest must never decrease TotalAssets
                let assets_after = client.get_total_assets();
                assert!(
                    assets_after >= assets_before_harvest,
                    "harvest reduced TotalAssets: before={assets_before_harvest} after={assets_after} (step {i})"
                );
            }
            // ── rebalance ──────────────────────────────────────────────────
            _ => {
                let expected_apy: i128 = 500; // 5% in bps
                client.rebalance(&protocols[proto_idx], &expected_apy, &min_out);
            }
        }));

        match result {
            Ok(()) => assert_vault_invariants(&client, &user),
            Err(payload) => {
                let msg = payload
                    .downcast_ref::<&str>()
                    .copied()
                    .or_else(|| payload.downcast_ref::<String>().map(|s| s.as_str()))
                    .unwrap_or("unknown panic");
                assert!(
                    is_allowed_panic(msg),
                    "unexpected panic at step {i} (op={op}): {msg}"
                );
            }
        }
    }
});
