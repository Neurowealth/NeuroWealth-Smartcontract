//! LibFuzzer harness: withdrawal-queue share accounting (Issue #777).
//!
//! Drives `share_math::queue::QueueModel` through arbitrary sequences of
//! deposit / queue / cancel / process / yield / loss / liquidity-recall
//! operations. After every step it asserts:
//!
//! 1. `total_shares == sum(free_shares + escrowed_shares)`
//! 2. per-user escrow equals the shares locked by that user's pending requests
//! 3. no negative balances; `0 <= idle_assets <= total_assets`
//! 4. pending requests never exceed `max_size`
//! 5. strict FIFO: no pending request is older than a fulfilled one
//! 6. payouts never exceed the requested assets
//!
//! and per operation: queueing/cancelling leave totals unchanged, cancel
//! restores the exact escrow and cannot repeat, processing pays out exactly
//! the assets that leave the vault, never dilutes remaining holders, and
//! never mutates terminal requests.
//!
//! The input format is documented on `share_math::queue::run_fuzz_input`.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    share_math::queue::run_fuzz_input(data);
});
