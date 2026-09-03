# @neurowealth/vault-client

Auto-generated TypeScript client for the **NeuroWealth Vault** Soroban smart contract on Stellar.

> **Do not edit `src/generated/vault.ts` by hand.**
> It is produced by `scripts/generate-client.js` from `contract-spec.json`.
> Run `node scripts/generate-client.js` from the repo root to regenerate.

---

## Installation

```bash
npm install @neurowealth/vault-client @stellar/stellar-sdk
# or
yarn add @neurowealth/vault-client @stellar/stellar-sdk
```

`@stellar/stellar-sdk` ≥ 12.0.0 is a peer dependency and must be installed separately.

---

## Quick start

```typescript
import * as StellarSdk from '@stellar/stellar-sdk';
import { VaultClient, DECIMAL_PLACES } from '@neurowealth/vault-client';

const client = new VaultClient({
  contractId: 'C...YOUR_CONTRACT_ADDRESS',
  rpcUrl: 'https://soroban-testnet.stellar.org',
  networkPassphrase: StellarSdk.Networks.TESTNET,
});
```

---

## Usage examples

### Query: `get_balance`

Read-only functions only need a `sourcePublicKey` — no signing required.

```typescript
const userAddress = 'GABC...';

// get_balance returns bigint (i128 maps to bigint)
const balance: bigint = await client.get_balance(userAddress, userAddress);

// Convert to human-readable USDC (7 decimals)
const usdc = Number(balance) / 10 ** DECIMAL_PLACES;
console.log(`Balance: ${usdc} USDC`);
```

### Mutation: `deposit`

State-changing functions require a `StellarSdk.Keypair` as the first argument.
The client builds, simulates, signs, submits, and polls for confirmation automatically.

```typescript
const signer = StellarSdk.Keypair.fromSecret('S...');

// Deposit 100 USDC (100 * 10^7 base units)
const amount = 100n * 10n ** BigInt(DECIMAL_PLACES); // 1_000_000_000n

const { hash } = await client.deposit(
  signer,
  signer.publicKey(), // user address
  amount,
);

console.log(`Deposit confirmed — tx hash: ${hash}`);
```

### Mutation: `withdraw`

```typescript
const signer = StellarSdk.Keypair.fromSecret('S...');

// Withdraw 50 USDC
const amount = 50n * 10n ** BigInt(DECIMAL_PLACES);

const { hash, result } = await client.withdraw(
  signer,
  signer.publicKey(), // user address
  amount,
);

console.log(`Withdrew ${result} base units — tx hash: ${hash}`);
```

### Query: `get_user_info`

```typescript
import { UserInfo } from '@neurowealth/vault-client';

const info: UserInfo = await client.get_user_info(userAddress, userAddress);
console.log('Shares:', info.shares);
console.log('Balance:', info.balance);
```

### Agent: `rebalance`

```typescript
const agentKeypair = StellarSdk.Keypair.fromSecret('S...');

await client.rebalance(
  agentKeypair,
  'blend',      // protocol: "blend" | "none"
  500_0000000n, // expected_apy: 500 bps * 10^7
  990_0000000n, // min_out: slippage floor
);
```

---

## API reference

All 65 contract functions are available as methods on `VaultClient`. They fall into two categories:

### Query methods (read-only)

Signature: `method(...contractParams, sourcePublicKey: string): Promise<ReturnType>`

| Method | Returns | Description |
|---|---|---|
| `get_balance` | `bigint` | User USDC balance |
| `get_shares` | `bigint` | User vault shares |
| `get_total_deposits` | `bigint` | Total deposited USDC |
| `get_total_assets` | `bigint` | Principal + accrued yield |
| `get_total_shares` | `bigint` | Total shares outstanding |
| `get_exchange_rate` | `bigint` | Assets per share × 10⁷ |
| `get_idle_balance` | `bigint` | Idle USDC held directly in contract |
| `get_deployed_assets` | `bigint` | USDC deployed in active yield protocol |
| `get_asset_breakdown` | `[bigint, bigint]` | Tuple of (idle, deployed) USDC |
| `get_owner` | `string` | Contract owner address |
| `get_agent` | `string` | Authorized agent address |
| `get_pending_agent_update` | `unknown \| null` | Pending agent update proposal info |
| `get_pending_upgrade` | `unknown \| null` | Pending contract upgrade proposal info |
| `get_user_info` | `UserInfo` | Full user snapshot |
| `get_user_strategy` | `string` | User yield strategy preference |
| `is_paused` | `boolean` | Vault pause state |
| `get_tvl_cap` | `bigint` | Current TVL cap |
| `get_user_deposit_cap` | `bigint` | Per-user deposit cap |
| `get_min_deposit` | `bigint` | Min per-tx deposit |
| `get_max_deposit` | `bigint` | Max per-tx deposit |
| `get_version` | `number` | Contract version |
| `get_current_protocol` | `string` | Active yield protocol |
| `get_blend_pool` | `string \| null` | Blend pool address |
| `get_dex_pool` | `string \| null` | DEX pool address |
| `get_pending_owner` | `string \| null` | Pending owner address |
| `preview_deposit_to_shares` | `bigint` | Preview shares from assets |
| `preview_shares_to_assets` | `bigint` | Preview assets from shares |
| `preview_withdraw` | `bigint` | Preview shares burned |
| `convert_to_shares` | `bigint` | Assets → shares |
| `convert_to_assets` | `bigint` | Shares → assets |
| `touch_user_ttl` | `boolean` | Extend user TTL |
| `get_blend_approval_ttl` | `number` | Blend approval TTL |
| `get_approval_ttl` | `number` | Token approval TTL |
| `get_rebalance_cooldown` | `number` | Rebalance cooldown |
| `get_last_rebalance_ledger` | `number` | Last rebalance ledger |
| `get_usdc_token` | `string` | USDC token address |

### Mutation methods (require signer Keypair)

Signature: `method(signer: Keypair, ...contractParams): Promise<TxResult<ReturnType>>`

| Method | Access | Description |
|---|---|---|
| `initialize` | once | Deploy vault |
| `deposit` | public | Deposit USDC, receive shares |
| `withdraw` | public | Burn shares, receive USDC |
| `withdraw_all` | public | Redeem all shares |
| `set_user_strategy` | public | Set user investment strategy preference |
| `rebalance` | agent-only | Reallocate to yield protocol |
| `update_total_assets` | agent-only | Sync yield accounting |
| `pause` | owner-only | Emergency pause |
| `unpause` | owner-only | Resume operations |
| `emergency_pause` | owner-only | Immediate pause (no sig check) |
| `set_tvl_cap` | owner-only | Update TVL cap |
| `set_user_deposit_cap` | owner-only | Update per-user cap |
| `set_caps` | owner-only | Update both caps atomically |
| `set_deposit_limits` | owner-only | Update min/max per tx |
| `set_blend_pool` | owner-only | Configure Blend pool |
| `set_dex_pool` | owner-only | Configure DEX pool |
| `set_blend_approval_ttl` | owner-only | Set Blend approval TTL |
| `set_approval_ttl` | owner-only | Set token approval TTL |
| `set_rebalance_cooldown` | owner-only | Set rebalance cooldown |
| `update_agent` | owner-only | Propose agent update (timelock step 1) |
| `confirm_agent_update` | owner-only | Confirm agent update (timelock step 2) |
| `cancel_agent_update` | owner-only | Cancel pending proposed agent update |
| `transfer_ownership` | owner-only | Initiate 2-step transfer |
| `accept_ownership` | pending-owner | Complete transfer |
| `cancel_ownership_transfer` | owner-only | Cancel pending transfer |
| `schedule_upgrade` | owner-only | Schedule contract WASM code upgrade (timelock step 1) |
| `execute_upgrade` | owner-only | Execute scheduled upgrade (timelock step 2) |
| `cancel_upgrade` | owner-only | Cancel pending contract upgrade proposal |
| `upgrade` | owner-only | Upgrade contract WASM |
| `set_limits` | owner-only | ⚠️ Deprecated — use `set_caps` |

---

## Types

### `UserInfo`

```typescript
interface UserInfo {
  address: string;       // Wallet address
  balance: bigint;       // USDC balance (7 decimals)
  shares: bigint;        // Vault shares owned
  deposit_time: bigint;  // Timestamp of first deposit
}
```

### `TxResult<T>`

```typescript
interface TxResult<T> {
  result?: T;            // Decoded return value
  hash?: string;         // Transaction hash
  simulation?: SorobanRpc.Api.SimulateTransactionResponse;
}
```

### `VaultErrorCode`

```typescript
const VaultErrorCode = {
  NegativeMin:             1,
  NegativeMax:             2,
  MaxLessThanMin:          3,
  ValidationError:         100,
  PausedError:             101,
  UnauthorizedAgentError:  102,
  UnauthorizedOwnerError:  103,
  InsufficientBalanceError:104,
  InvalidAmountError:      105,
  DepositCapExceededError: 106,
  TvlCapExceededError:     107,
  SlippageError:           108,
} as const;
```

### Constants

```typescript
const DEFAULT_USER_DEPOSIT_CAP: bigint; // 10,000 USDC
const DEFAULT_MIN_DEPOSIT:      bigint; // 1 USDC
const DEFAULT_MAX_DEPOSIT:      bigint; // 1,000 USDC
const DECIMAL_PLACES:           number; // 7
```

---

## Event types

All 34 contract events are exported as TypeScript interfaces (e.g. `DepositEvent`, `RebalanceEvent`, `AssetsUpdatedEvent`). Use them when parsing Soroban event streams:

```typescript
import type { DepositEvent } from '@neurowealth/vault-client';

function handleDepositEvent(raw: unknown): DepositEvent {
  // raw is already decoded by stellar-sdk's event parser
  return raw as DepositEvent;
}
```

### Event Listener

`VaultEventListener` wraps Soroban RPC event streaming with type-safe handlers for each event:

```typescript
import { VaultEventListener } from '@neurowealth/vault-client';
import * as StellarSdk from '@stellar/stellar-sdk';

const server = new StellarSdk.SorobanRpc.Server('https://soroban-testnet.stellar.org');

const listener = new VaultEventListener({
  contractId: 'C...YOUR_CONTRACT_ADDRESS',
  server,
  networkPassphrase: StellarSdk.Networks.TESTNET,
});

// Type-safe handlers for each event
listener.onDeposit((event) => {
  console.log(`Deposit: ${event.user} deposited ${event.amount} USDC`);
});

listener.onWithdraw((event) => {
  console.log(`Withdraw: ${event.user} withdrew ${event.amount} USDC`);
});

listener.onRebalance((event) => {
  console.log(`Rebalance: ${event.protocol} status=${event.status}`);
});

listener.onRebalanceFailed((event) => {
  console.log(`Rebalance failed: ${event.from_protocol} — ${event.reason}`);
});

listener.onAgentUpdateProposed((event) => {
  console.log(`Agent update proposed: ${event.old_agent} -> ${event.new_agent}`);
});

listener.onUpgraded((event) => {
  console.log(`Upgraded: v${event.old_version} -> v${event.new_version}`);
});

// Start listening (polls every 5 seconds)
await listener.start();

// Filter by user address
listener.onDeposit((event) => {
  console.log(`User deposit: ${event.amount}`);
}, 'GABC...USER_ADDRESS');

// Stop when done
listener.stop();
```

**Available handler methods:**

| Method | Event Type | User Filter |
|---|---|---|
| `onDeposit` | `DepositEvent` | Optional |
| `onWithdraw` | `WithdrawEvent` | Optional |
| `onRebalance` | `RebalanceEvent` | - |
| `onRebalanceFailed` | `RebalanceFailedEvent` | - |
| `onProtocolChanged` | `ProtocolChangedEvent` | - |
| `onInitialized` | `VaultInitializedEvent` | - |
| `onPaused` | `VaultPausedEvent` | - |
| `onUnpaused` | `VaultUnpausedEvent` | - |
| `onEmergencyPaused` | `EmergencyPausedEvent` | - |
| `onTvlCapUpdated` | `TvlCapUpdatedEvent` | - |
| `onUserDepositCapUpdated` | `UserDepositCapUpdatedEvent` | - |
| `onCapsUpdated` | `CapsUpdatedEvent` | - |
| `onLimitsUpdated` | `LimitsUpdatedEvent` | - |
| `onDepositLimitsUpdated` | `DepositLimitsUpdatedEvent` | - |
| `onAgentUpdated` | `AgentUpdatedEvent` | - |
| `onAgentUpdateProposed` | `AgentUpdateProposedEvent` | - |
| `onAgentUpdateConfirmed` | `AgentUpdateConfirmedEvent` | - |
| `onAgentUpdateCancelled` | `AgentUpdateCancelledEvent` | - |
| `onOwnershipInitiated` | `OwnershipTransferInitiatedEvent` | - |
| `onOwnershipTransferred` | `OwnershipTransferredEvent` | - |
| `onOwnershipCancelled` | `OwnershipTransferCancelledEvent` | - |
| `onAssetsUpdated` | `AssetsUpdatedEvent` | - |
| `onUpgraded` | `UpgradedEvent` | - |
| `onUpgradeScheduled` | `UpgradeScheduledEvent` | - |
| `onUpgradeCancelled` | `UpgradeCancelledEvent` | - |
| `onBlendSupply` | `BlendSupplyEvent` | - |
| `onBlendWithdraw` | `BlendWithdrawEvent` | - |
| `onBlendPoolConfigured` | `BlendPoolConfiguredEvent` | - |
| `onDexSupply` | `DexSupplyEvent` | - |
| `onDexWithdraw` | `DexWithdrawEvent` | - |
| `onDexPoolConfigured` | `DexPoolConfiguredEvent` | - |
| `onUserStrategyUpdated` | `UserStrategyUpdatedEvent` | Optional |

---

## Regenerating the client

The generated file must stay in sync with `contract-spec.json`. Run the generator from the repo root:

```bash
node scripts/generate-client.js
```

CI will fail on a pull request if the spec changes but the generated client is not updated. See `.github/workflows/contract-spec.yml`.

---

## Project structure

```
packages/vault-client/
├── package.json
├── tsconfig.json
├── README.md
└── src/
    ├── index.ts                  ← public barrel export
    ├── event-listener.ts         ← VaultEventListener for Soroban event streaming
    └── generated/
        └── vault.ts              ← AUTO-GENERATED, do not edit
```

## Rate-limit APIs

The generated client exposes `set_rate_limit(category, max_calls,
window_ledgers)` and `get_rate_limit(category)` for the owner-configurable
fixed-window policies. Supported categories are `deposit`, `withdraw`,
`rebalance`, `touch_ttl`, `preview`, and `batch_dep`. It also exposes
`get_global_rate_limit_state`, `get_user_rate_limit_state`, and
`set_max_batch_size`/`get_max_batch_size`.

The preview and conversion entrypoints consume the global `preview` bucket when
invoked as transactions. RPC simulation does not commit a bucket, so it is
suitable for display but cannot be used to measure committed call usage.
