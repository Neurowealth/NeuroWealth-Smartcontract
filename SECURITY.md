# Security Model

This document describes the security architecture, trust model, and threat model for the NeuroWealth Vault contract.

## Trust Model

The NeuroWealth Vault implements a partitioned trust model with three distinct roles:

### Owner

The contract owner has the following permissions:
- **Pause/Unpause**: Can halt all deposits and withdrawals during emergencies
- **Set TVL Cap**: Can limit total deposits to manage risk exposure
- **Set User Deposit Cap**: Can limit per-user exposure
- **Upgrade Contract**: Can upgrade contract code (Phase 2)

The owner **CANNOT**:
- Access user funds directly
- Withdraw funds from user accounts
- Change the agent address after initialization
- Modify user balances

### AI Agent

The authorized AI agent has the following permissions:
- **Rebalance**: Can call `rebalance()` to signal strategy changes
- **Read Access**: Can read all vault state to make yield decisions

The agent **CANNOT**:
- Withdraw user funds directly to itself
- Change vault configuration
- Access USDC tokens directly (must go through vault)
- Modify user balances
- Pause/unpause the vault

### Users

Regular users have the following permissions:
- **Deposit**: Can deposit USDC into the vault
- **Withdraw**: Can withdraw their own USDC at any time
- **Read**: Can query their balance and vault state

Users **CANNOT**:
- Access other users' funds
- Manipulate vault configuration
- Call agent-only functions

## Withdrawal Guarantees

### Immediate Withdrawals

Users can withdraw their USDC at any time without:
- Lock-up periods
- Withdrawal fees
- Approval requirements beyond their signature

The vault maintains sufficient USDC on-hand to process withdrawals because:
1. Total deposits (`TotalDeposits`) tracks all USDC held by the vault
2. Yield deployed externally is tracked separately by the AI agent
3. The agent only deploys yield (not principal) to external protocols

### Withdrawal Priority

In the current implementation:
- User withdrawals are always processed from vault-held USDC
- If the vault has insufficient USDC, the withdrawal will fail (by design)
- The AI agent is responsible for maintaining sufficient liquidity

**Future (Phase 2)**: Will implement withdrawal queue for scenarios where vault USDC is fully deployed.

## Upgrade Risks

### Contract Upgrade Process

The contract owner can upgrade the contract code while preserving storage state. This introduces:

1. **Single Point of Failure**: The owner key becomes a high-value target
2. **Persistence of Storage**: Old bugs in storage layout could be exploited
3. **Migration Risk**: Data migration during upgrades could fail

### Mitigation Measures

- Owner key should be a multi-sig or hardware wallet
- Upgrades should go through a timelock (future)
- Storage layout changes should be carefully tested
- Emergency pause available before upgrades

## Known Assumptions

### 1. USDC Token Assumptions

- USDC token is non-malicious (standard Stellar token)
- USDC has 7 decimal places
- Transfer always succeeds for valid inputs (or reverts on failure)

### 2. Agent Behavior Assumptions

- Agent will only deploy yield (not principal) to external protocols
- Agent will maintain sufficient liquidity for withdrawals
- Agent will not collude with owner to steal funds

### 3. Market Assumptions

- USDC remains pegged to $1.00
- External yield protocols (Blend, DEX) remain operational
- No flash loan attacks (Stellar doesn't support them)

### 4. Operational Assumptions

- Owner keys are kept secure
- Agent keys are kept secure
- RPC endpoints are reliable

## Threat Model

### Threat: Owner Compromise

**Scenario**: Attackers gain control of the owner key

**Impact**:
- Attacker can pause the vault (denial of service)
- Attacker can set TVL cap to 0 (denial of service)
- Attacker CANNOT steal user funds

**Mitigation**:
- Use hardware wallet or multi-sig for owner
- Implement timelock for critical actions (Phase 2)

### Threat: Agent Compromise

**Scenario**: Attackers gain control of the agent key

**Impact**:
- Attacker can emit rebalance events (signal only)
- Attacker CANNOT directly steal funds
- Attacker CANNOT access vault USDC

**Mitigation**:
- Agent key should be hot wallet with limited permissions
- Agent can only call rebalance(), cannot withdraw

### Threat: User Griefing

**Scenario**: User deposits and immediately withdraws to waste gas

**Impact**:
- Minimal - only affects the user's own funds
- Gas wasted by user

**Mitigation**:
- None required - economic inefficiency borne by user

### Threat: Front-Running

**Scenario**: Observer sees deposit and front-runs with rebalance

**Impact**:
- Minimal in current design
- Rebalance is a signal, not a direct value transfer

**Mitigation**:
- Phase 2 will add slippage protection

## Access Control Summary

| Function | Owner | Agent | User | Anyone |
|----------|-------|-------|------|--------|
| initialize | - | - | - | Deployer |
| deposit | - | - | ✅ | - |
| withdraw | - | - | ✅ | - |
| rebalance | - | ✅ | - | - |
| pause | ✅ | - | - | - |
| unpause | ✅ | - | - | - |
| set_tvl_cap | ✅ | - | - | - |
| set_user_deposit_cap | ✅ | - | - | - |
| get_balance | - | ✅ | ✅ | ✅ |
| get_total_deposits | - | ✅ | ✅ | ✅ |
| get_agent | - | ✅ | ✅ | ✅ |
| is_paused | - | ✅ | ✅ | ✅ |

## Security Best Practices Implemented

1. **Checks-Effects-Interactions Pattern**: All state updates happen before external calls
2. **Auth on Withdrawals**: `require_auth()` ensures users can only access their own funds
3. **Minimum Deposits**: Prevents dust attacks
4. **Deposit Caps**: Limits exposure per user
5. **TVL Caps**: Limits total exposure
6. **Pausable**: Emergency stop functionality

## Audit Checklist

Before any mainnet deployment, ensure:

- [ ] All functions have documented panic conditions
- [ ] All state changes emit events
- [ ] Access control verified for each function
- [ ] Upgrade mechanism tested on testnet
- [ ] Pause/unpause tested
- [ ] Withdrawal flow tested with edge cases
- [ ] Maximum deposit limits enforced
- [ ] TVL cap enforced
- [ ] Integration with USDC token tested
- [ ] Integration with Blend protocol tested (Phase 2)
