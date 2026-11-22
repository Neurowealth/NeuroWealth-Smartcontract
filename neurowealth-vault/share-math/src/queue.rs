//! Withdrawal-queue share accounting model (Issue #777).
//!
//! Reference model for a FIFO withdrawal queue layered on top of the vault's
//! share math, in the shape exercised by `tests/test_withdrawal_queue.rs`
//! (`queue_withdrawal` / `cancel_withdrawal_request` /
//! `process_withdrawal_queue` with a `max_size` queue config).
//!
//! Accounting rules:
//! - **Queue**: a user asks for `assets`. `shares_ceil(assets)` shares move
//!   from the user's free balance into escrow. `total_shares` is unchanged —
//!   escrowed shares still exist and keep earning (or losing) with the pool.
//! - **Cancel**: the exact escrowed shares return to the owner.
//! - **Process**: pending requests are fulfilled strictly in id order while
//!   idle liquidity covers the head request. The payout is
//!   `min(requested, assets_from_shares(escrowed))` at the *fulfilment* rate;
//!   `shares_ceil(payout)` escrowed shares are burned and any remainder is
//!   refunded to the owner's free balance.
//!
//! [`run_fuzz_input`] decodes an arbitrary byte string into an operation
//! sequence and asserts every invariant after each step. It is shared by the
//! `withdrawal_queue_invariants` cargo-fuzz target and the stable
//! randomized test below, so both exercise identical checks.

use crate::{assets_from_shares, rate_non_decreasing, shares_ceil, shares_floor};

/// Number of users tracked by the model.
pub const USERS: usize = 3;
/// Hard capacity of the request log (ids are never reused).
pub const QUEUE_CAPACITY: usize = 16;

/// Lifecycle of a withdrawal request. `Fulfilled` and `Cancelled` are terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestStatus {
    Pending,
    Fulfilled,
    Cancelled,
}

/// One queued withdrawal request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Request {
    pub user: usize,
    /// Assets the user asked for.
    pub assets: i128,
    /// Shares moved into escrow when the request was queued.
    pub escrowed: i128,
    /// Assets actually paid on fulfilment (0 until fulfilled).
    pub paid: i128,
    pub status: RequestStatus,
}

const EMPTY_REQUEST: Request = Request {
    user: 0,
    assets: 0,
    escrowed: 0,
    paid: 0,
    status: RequestStatus::Cancelled,
};

/// Vault state plus the withdrawal queue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QueueModel {
    pub total_shares: i128,
    pub total_assets: i128,
    /// Assets held by the vault itself (not deployed to a protocol).
    pub idle_assets: i128,
    /// Shares each user can freely use (excludes escrow).
    pub free_shares: [i128; USERS],
    /// Shares each user has locked in pending requests.
    pub escrowed_shares: [i128; USERS],
    /// Cumulative assets paid out to each user by the queue.
    pub paid_out: [i128; USERS],
    /// Maximum number of simultaneously pending requests.
    pub max_pending: usize,
    pub requests: [Request; QUEUE_CAPACITY],
    /// Number of request ids issued so far (request id = index + 1).
    pub len: usize,
}

impl QueueModel {
    /// Empty vault with the given pending-request limit.
    #[must_use]
    pub const fn new(max_pending: usize) -> Self {
        Self {
            total_shares: 0,
            total_assets: 0,
            idle_assets: 0,
            free_shares: [0; USERS],
            escrowed_shares: [0; USERS],
            paid_out: [0; USERS],
            max_pending,
            requests: [EMPTY_REQUEST; QUEUE_CAPACITY],
            len: 0,
        }
    }

    /// Number of requests still pending.
    #[must_use]
    pub fn pending_count(&self) -> usize {
        self.requests[..self.len]
            .iter()
            .filter(|r| r.status == RequestStatus::Pending)
            .count()
    }

    /// Deposit `assets` for `user` (floor mint); the assets land as idle liquidity.
    #[must_use]
    pub fn deposit(self, user: usize, assets: i128) -> Option<Self> {
        if user >= USERS || assets <= 0 {
            return None;
        }
        let minted = shares_floor(assets, self.total_shares, self.total_assets)?;
        if minted <= 0 {
            return None;
        }
        let mut next = self;
        next.free_shares[user] = next.free_shares[user].checked_add(minted)?;
        next.total_shares = next.total_shares.checked_add(minted)?;
        next.total_assets = next.total_assets.checked_add(assets)?;
        next.idle_assets = next.idle_assets.checked_add(assets)?;
        Some(next)
    }

    /// Protocol yield: deployed assets grow, no shares are minted.
    #[must_use]
    pub fn accrue_yield(self, amount: i128) -> Option<Self> {
        if amount < 0 {
            return None;
        }
        let mut next = self;
        next.total_assets = next.total_assets.checked_add(amount)?;
        Some(next)
    }

    /// Protocol loss: total assets shrink (idle liquidity is capped to match).
    #[must_use]
    pub fn realize_loss(self, amount: i128) -> Option<Self> {
        if amount < 0 || amount > self.total_assets {
            return None;
        }
        let mut next = self;
        next.total_assets = next.total_assets.checked_sub(amount)?;
        next.idle_assets = next.idle_assets.min(next.total_assets);
        Some(next)
    }

    /// Agent recalls up to `amount` of deployed assets into idle liquidity.
    #[must_use]
    pub fn recall_liquidity(self, amount: i128) -> Option<Self> {
        if amount < 0 {
            return None;
        }
        let deployed = self.total_assets.checked_sub(self.idle_assets)?;
        let mut next = self;
        next.idle_assets = next.idle_assets.checked_add(amount.min(deployed))?;
        Some(next)
    }

    /// Queue a withdrawal of `assets` for `user`, escrowing ceil-rounded shares.
    /// Returns the new state and the 1-based request id.
    #[must_use]
    pub fn queue_withdrawal(self, user: usize, assets: i128) -> Option<(Self, u32)> {
        if user >= USERS || assets <= 0 || self.len >= QUEUE_CAPACITY {
            return None;
        }
        if self.pending_count() >= self.max_pending {
            return None;
        }
        let escrow = shares_ceil(assets, self.total_shares, self.total_assets)?;
        if escrow <= 0 || escrow > self.free_shares[user] {
            return None;
        }
        let mut next = self;
        next.free_shares[user] = next.free_shares[user].checked_sub(escrow)?;
        next.escrowed_shares[user] = next.escrowed_shares[user].checked_add(escrow)?;
        next.requests[next.len] = Request {
            user,
            assets,
            escrowed: escrow,
            paid: 0,
            status: RequestStatus::Pending,
        };
        next.len += 1;
        Some((next, u32::try_from(next.len).ok()?))
    }

    /// Cancel a pending request; only its owner may cancel.
    #[must_use]
    pub fn cancel(self, caller: usize, id: u32) -> Option<Self> {
        let idx = usize::try_from(id).ok()?.checked_sub(1)?;
        if idx >= self.len {
            return None;
        }
        let req = self.requests[idx];
        if req.status != RequestStatus::Pending || req.user != caller {
            return None;
        }
        let mut next = self;
        next.escrowed_shares[req.user] =
            next.escrowed_shares[req.user].checked_sub(req.escrowed)?;
        next.free_shares[req.user] = next.free_shares[req.user].checked_add(req.escrowed)?;
        next.requests[idx].status = RequestStatus::Cancelled;
        Some(next)
    }

    /// Fulfil up to `max` pending requests in FIFO order. Stops at the first
    /// request idle liquidity cannot cover (no queue-jumping). Returns the
    /// new state and the number of requests fulfilled.
    #[must_use]
    pub fn process(self, max: u32) -> Option<(Self, u32)> {
        let mut next = self;
        let mut processed = 0_u32;
        for idx in 0..next.len {
            if processed >= max {
                break;
            }
            let req = next.requests[idx];
            if req.status != RequestStatus::Pending {
                continue;
            }
            let value = assets_from_shares(req.escrowed, next.total_shares, next.total_assets)?;
            let payout = req.assets.min(value);
            if payout > next.idle_assets {
                break;
            }
            let burn = shares_ceil(payout, next.total_shares, next.total_assets)?.min(req.escrowed);
            let refund = req.escrowed.checked_sub(burn)?;

            next.escrowed_shares[req.user] =
                next.escrowed_shares[req.user].checked_sub(req.escrowed)?;
            next.free_shares[req.user] = next.free_shares[req.user].checked_add(refund)?;
            next.total_shares = next.total_shares.checked_sub(burn)?;
            next.total_assets = next.total_assets.checked_sub(payout)?;
            next.idle_assets = next.idle_assets.checked_sub(payout)?;
            next.paid_out[req.user] = next.paid_out[req.user].checked_add(payout)?;
            next.requests[idx].paid = payout;
            next.requests[idx].status = RequestStatus::Fulfilled;
            processed += 1;
        }
        Some((next, processed))
    }

    /// Asserts every state invariant.
    ///
    /// # Panics
    ///
    /// Panics with a description of the first violated invariant.
    pub fn assert_invariants(&self) {
        // Share conservation: free + escrowed shares account for every share.
        let mut sum: i128 = 0;
        for u in 0..USERS {
            assert!(self.free_shares[u] >= 0, "free shares went negative");
            assert!(
                self.escrowed_shares[u] >= 0,
                "escrowed shares went negative"
            );
            assert!(self.paid_out[u] >= 0, "paid-out total went negative");
            sum = sum
                .checked_add(self.free_shares[u])
                .and_then(|s| s.checked_add(self.escrowed_shares[u]))
                .expect("share sum overflow");
        }
        assert_eq!(
            sum, self.total_shares,
            "total_shares != sum(free + escrowed)"
        );

        // Escrow per user equals the shares locked by that user's pending requests.
        let mut locked = [0_i128; USERS];
        for req in &self.requests[..self.len] {
            assert!(req.user < USERS, "request owner out of range");
            assert!(req.escrowed > 0, "request escrowed no shares");
            match req.status {
                RequestStatus::Pending => {
                    locked[req.user] += req.escrowed;
                    assert_eq!(req.paid, 0, "pending request already paid");
                }
                RequestStatus::Fulfilled => {
                    assert!(req.paid >= 0, "negative payout");
                    assert!(req.paid <= req.assets, "payout exceeds requested assets");
                }
                RequestStatus::Cancelled => assert_eq!(req.paid, 0, "cancelled request was paid"),
            }
        }
        assert_eq!(
            locked, self.escrowed_shares,
            "escrow ledger drifted from pending requests"
        );

        // Queue bounds.
        assert!(self.len <= QUEUE_CAPACITY, "request log overflowed");
        assert!(
            self.pending_count() <= self.max_pending,
            "pending requests exceed max_size"
        );

        // FIFO: no pending request may be older than a fulfilled one.
        if let Some(last_fulfilled) = self.requests[..self.len]
            .iter()
            .rposition(|r| r.status == RequestStatus::Fulfilled)
        {
            assert!(
                self.requests[..last_fulfilled]
                    .iter()
                    .all(|r| r.status != RequestStatus::Pending),
                "a newer request was fulfilled ahead of an older pending one"
            );
        }

        // Asset bounds.
        assert!(self.total_assets >= 0, "total_assets went negative");
        assert!(
            self.idle_assets >= 0 && self.idle_assets <= self.total_assets,
            "idle liquidity outside [0, total_assets]"
        );
    }
}

fn amount(bytes: [u8; 2]) -> i128 {
    i128::from(u16::from_le_bytes(bytes))
}

/// Queueing only moves shares into escrow, and the escrow is worth at least
/// the requested assets (ceil-rounded shares, floor-rounded value).
fn check_queue(before: &QueueModel, next: &QueueModel, id: u32) {
    let req = next.requests[next.len - 1];
    assert_eq!(
        usize::try_from(id).ok(),
        Some(next.len),
        "request ids must be sequential"
    );
    let value = assets_from_shares(req.escrowed, before.total_shares, before.total_assets)
        .expect("escrow value overflow");
    assert!(
        before.total_shares == 0 || before.total_assets == 0 || value >= req.assets,
        "escrow worth less than the requested assets"
    );
    assert_eq!(
        next.total_shares, before.total_shares,
        "queueing changed total_shares"
    );
    assert_eq!(
        next.total_assets, before.total_assets,
        "queueing changed total_assets"
    );
}

/// Only the owner can cancel, cancelling restores the exact escrow, and a
/// request cannot be cancelled twice.
fn check_cancel(before: &QueueModel, next: &QueueModel, caller: usize, id: u32) {
    let req = before.requests[usize::try_from(id).expect("id fits usize") - 1];
    assert_eq!(req.user, caller, "non-owner cancelled a request");
    assert_eq!(
        next.free_shares[caller],
        before.free_shares[caller] + req.escrowed,
        "cancel did not restore the exact escrowed shares"
    );
    assert_eq!(
        next.total_shares, before.total_shares,
        "cancel changed total_shares"
    );
    assert_eq!(
        next.total_assets, before.total_assets,
        "cancel changed total_assets"
    );
    assert!(next.cancel(caller, id).is_none(), "request cancelled twice");
}

/// Processing pays out exactly what leaves the vault, never dilutes the
/// remaining holders, and never touches terminal requests.
fn check_process(before: &QueueModel, next: &QueueModel, max: u32, processed: u32) {
    assert!(processed <= max, "processed more requests than asked");
    let paid: i128 = (0..USERS)
        .map(|u| next.paid_out[u] - before.paid_out[u])
        .sum();
    assert_eq!(
        before.total_assets - next.total_assets,
        paid,
        "assets left the vault without being paid to a requester"
    );
    assert_eq!(
        rate_non_decreasing(
            before.total_assets,
            before.total_shares,
            next.total_assets,
            next.total_shares
        ),
        Some(true),
        "fulfilling withdrawals diluted remaining shareholders"
    );
    for (old, new) in before.requests[..before.len]
        .iter()
        .zip(&next.requests[..next.len])
    {
        if old.status != RequestStatus::Pending {
            assert_eq!(old, new, "a terminal request was modified");
        }
    }
}

/// Decode `data` into an operation sequence and check invariants after every
/// step, plus per-operation transition properties.
///
/// Each operation is a 4-byte chunk: `[op, user/arg, amount_lo, amount_hi]`.
/// Amounts are scaled so yields, losses and rounding at small pool sizes are
/// all reachable.
///
/// # Panics
///
/// Panics when any invariant or transition property is violated — that is
/// the signal the fuzzer looks for.
pub fn run_fuzz_input(data: &[u8]) {
    let max_pending = data.first().map_or(4, |b| usize::from(b % 8) + 1);
    let mut vault = QueueModel::new(max_pending);

    let (chunks, _remainder) = data.as_chunks::<4>();
    for chunk in chunks {
        let (op, arg, amt) = (chunk[0] % 7, chunk[1], amount([chunk[2], chunk[3]]));
        let user = usize::from(arg) % USERS;
        let before = vault;

        let next = match op {
            0 => vault.deposit(user, amt.saturating_mul(1_000) + 1),
            1 => vault.queue_withdrawal(user, amt + 1).map(|(next, id)| {
                check_queue(&before, &next, id);
                next
            }),
            2 => {
                let issued = u32::try_from(vault.len).expect("len fits u32").max(1);
                let id = u32::from(arg) % issued + 1;
                vault
                    .cancel(user, id)
                    .inspect(|next| check_cancel(&before, next, user, id))
            }
            3 => {
                let max = u32::from(arg % 5);
                vault.process(max).map(|(next, processed)| {
                    check_process(&before, &next, max, processed);
                    next
                })
            }
            4 => vault.accrue_yield(amt),
            5 => vault.realize_loss(amt.min(vault.total_assets / 4)),
            _ => vault.recall_liquidity(amt.saturating_mul(1_000)),
        };
        if let Some(next) = next {
            vault = next;
        }

        vault.assert_invariants();
    }
}

#[cfg(test)]
mod tests {
    use super::{run_fuzz_input, QueueModel, RequestStatus};

    #[test]
    fn mirrors_contract_queue_scenario() {
        // Same flow as tests/test_withdrawal_queue.rs.
        let vault = QueueModel::new(5).deposit(0, 1_000).unwrap();
        let (vault, id1) = vault.queue_withdrawal(0, 200).unwrap();
        let (vault, id2) = vault.queue_withdrawal(0, 100).unwrap();
        assert_eq!((id1, id2), (1, 2));
        let vault = vault.cancel(0, id2).unwrap();
        let (vault, processed) = vault.process(10).unwrap();
        assert_eq!(processed, 1);
        assert_eq!(vault.requests[0].status, RequestStatus::Fulfilled);
        assert_eq!(vault.requests[1].status, RequestStatus::Cancelled);
        assert_eq!(vault.free_shares[0], 800);
        vault.assert_invariants();
    }

    #[test]
    fn head_of_line_blocks_until_liquidity_recalled() {
        let vault = QueueModel::new(4)
            .deposit(0, 1_000)
            .unwrap()
            .deposit(1, 1_000)
            .unwrap();
        // Deploy everything except 100 idle.
        let mut vault = vault;
        vault.idle_assets = 100;
        let (vault, _) = vault.queue_withdrawal(0, 500).unwrap();
        let (vault, _) = vault.queue_withdrawal(1, 50).unwrap();
        let (vault, processed) = vault.process(10).unwrap();
        assert_eq!(
            processed, 0,
            "smaller later request must not jump the queue"
        );
        let (vault, processed) = vault.recall_liquidity(1_000).unwrap().process(10).unwrap();
        assert_eq!(processed, 2);
        vault.assert_invariants();
    }

    #[test]
    fn loss_after_queueing_refunds_nothing_and_pays_escrow_value() {
        let vault = QueueModel::new(4)
            .deposit(0, 1_000)
            .unwrap()
            .deposit(1, 1_000)
            .unwrap();
        let (vault, _) = vault.queue_withdrawal(0, 1_000).unwrap();
        let vault = vault.realize_loss(1_000).unwrap();
        let (vault, processed) = vault.process(1).unwrap();
        assert_eq!(processed, 1);
        assert_eq!(
            vault.requests[0].paid, 500,
            "payout is capped at escrow value after a loss"
        );
        assert_eq!(vault.free_shares[0], 0);
        vault.assert_invariants();
    }

    #[test]
    fn yield_after_queueing_refunds_excess_escrow() {
        let vault = QueueModel::new(4).deposit(0, 1_000).unwrap();
        let (vault, _) = vault.queue_withdrawal(0, 500).unwrap();
        let vault = vault
            .accrue_yield(1_000)
            .unwrap()
            .recall_liquidity(1_000)
            .unwrap();
        let (vault, _) = vault.process(1).unwrap();
        assert_eq!(vault.requests[0].paid, 500);
        assert!(
            vault.free_shares[0] > 500,
            "excess escrow is refunded after yield"
        );
        vault.assert_invariants();
    }

    /// Stable-toolchain counterpart of the `withdrawal_queue_invariants`
    /// cargo-fuzz target: runs the same harness over pseudo-random inputs.
    #[test]
    fn randomized_queue_sequences_hold_invariants() {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        let mut buf = [0_u8; 256];
        for _ in 0..2_000 {
            for byte in &mut buf {
                // xorshift64*
                state ^= state >> 12;
                state ^= state << 25;
                state ^= state >> 27;
                *byte = u8::try_from(state.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 56).unwrap();
            }
            run_fuzz_input(&buf);
        }
    }
}
