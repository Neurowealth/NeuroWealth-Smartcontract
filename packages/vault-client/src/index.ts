/**
 * @neurowealth/vault-client
 *
 * Auto-generated typed bindings for the NeuroWealth Vault Soroban contract.
 * Re-export everything from the generated file so consumers import from one place.
 *
 * @example
 * import { VaultClient, UserInfo, VaultErrorCode, DECIMAL_PLACES } from '@neurowealth/vault-client';
 */

export {
  // Main client class
  VaultClient,

  // Option / result types
  type TxResult,
  type InvokeOptions,

  // Struct types
  type UserInfo,
  type RateLimitConfig,
  type RateLimitState,

  // Event payload interfaces
  type VaultInitializedEvent,
  type InitFailedEvent,
  type DepositEvent,
  type WithdrawEvent,
  type RebalanceEvent,
  type RebalanceFailedEvent,
  type ProtocolChangedEvent,
  type PauseEvent,
  type VaultPausedEvent,
  type VaultUnpausedEvent,
  type EmergencyPausedEvent,
  type TvlCapUpdatedEvent,
  type UserDepositCapUpdatedEvent,
  type CapsUpdatedEvent,
  type LimitsUpdatedEvent,
  type DepositLimitsUpdatedEvent,
  type AgentUpdatedEvent,
  type AgentUpdateProposedEvent,
  type AgentUpdateConfirmedEvent,
  type AgentUpdateCancelledEvent,
  type OwnershipTransferInitiatedEvent,
  type OwnershipTransferredEvent,
  type OwnershipTransferCancelledEvent,
  type AssetsUpdatedEvent,
  type UpgradedEvent,
  type UpgradeScheduledEvent,
  type UpgradeCancelledEvent,
  type BlendSupplyEvent,
  type BlendWithdrawEvent,
  type BlendPoolConfiguredEvent,
  type DexSupplyEvent,
  type DexWithdrawEvent,
  type DexPoolConfiguredEvent,
  type UserStrategyUpdatedEvent,
  type RateLimitConfigUpdatedEvent,
  type BatchSizeLimitUpdatedEvent,
  type RateLimitExceededEvent,

  // Error codes
  VaultErrorCode,
  VaultError,
  type VaultErrorCode as VaultErrorCodeType,

  // Constants
  DEFAULT_USER_DEPOSIT_CAP,
  DEFAULT_MIN_DEPOSIT,
  DEFAULT_MAX_DEPOSIT,
  DECIMAL_PLACES,
} from './generated/vault';

// Event listener
export { VaultEventListener } from './event-listener';
export type { EventListenerOptions, EventHandler } from './event-listener';
