mod test_access_control;
mod test_approval_ttl;
mod test_auth;
mod test_balance_shares_invariant;
#[cfg(feature = "blend-devnet")]
mod test_blend_devnet;
mod test_blend_integration;
mod test_budget;
mod test_checked_arithmetic;
mod test_deposit;
#[cfg(feature = "dex-devnet")]
mod test_dex_devnet;
mod test_dex_integration;
mod test_event_schema;
mod test_events;
mod test_exchange_rate;
mod test_fuzz_deposit_withdraw;
mod test_inflation_attack;
mod test_initialize;
mod test_legacy_inline;
mod test_limits;
mod test_math_limits;
mod test_pause;
mod test_rebalance;
mod test_rebalance_cooldown;
mod test_rebalance_integration;
mod test_rounding_math;
mod test_shares;
mod test_total_assets_cap;
mod test_ttl;
mod test_update_total_assets_blend;
mod test_withdraw;
mod test_yield;
mod test_user_strategy;
mod utils;

