# Liquidity Mining Rewards Integration

NeuroWealth Vault automatically claims and compounds liquidity mining rewards from integrated DeFi protocols (Blend Protocol lending rewards, DEX liquidity pool incentives) to maximize user yield.

---

## Architecture & Lifecycle

```
[ Deployed Vault Assets in Blend / DEX ]
                 ↓
      (Accrues reward tokens: BLND, DEX tokens)
                 ↓
  Step 1: Agent calls `claim_rewards(protocol_id)`
          → Emits `RewardsClaimedEvent { protocol, token, amount }`
                 ↓
  Step 2: Auto-swap reward tokens for USDC on Soroban DEX
          → Emits `RewardsSwappedEvent { token_in, amount_in, usdc_out, slippage_bps }`
                 ↓
  Step 3: Add swapped USDC to `TotalAssets`
          → Compounds vault exchange rate for all share holders
```

---

## Key Invariants & Safeguards

1. **Auto-Compounding**: Swapped proceeds are immediately added to vault reserves, increasing the value of every existing share (`TotalAssets / TotalSupply`).
2. **Slippage Bounds**: Swaps must adhere to maximum slippage bounds (default 100 bps / 1%) to prevent MEV exploitation.
3. **Gas Optimization**: Reward harvesting is batched during scheduled rebalancing loops to amortize transaction fees.
