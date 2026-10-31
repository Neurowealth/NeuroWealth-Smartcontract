/**
 * VaultEventListener — Type-safe Soroban event streaming for the NeuroWealth Vault.
 *
 * Wraps the Stellar SDK's Soroban RPC event streaming to provide
 * typed callbacks for each vault event. Supports filtering by topic
 * and user address.
 *
 * @example
 * ```typescript
 * import { VaultEventListener } from '@neurowealth/vault-client';
 * import * as StellarSdk from '@stellar/stellar-sdk';
 *
 * const server = new StellarSdk.rpc.Server('https://soroban-testnet.stellar.org');
 *
 * const listener = new VaultEventListener({
 *   contractId: 'C...',
 *   server,
 *   networkPassphrase: StellarSdk.Networks.TESTNET,
 * });
 *
 * listener.onDeposit((event) => {
 *   console.log(`Deposit: ${event.user} deposited ${event.amount}`);
 * });
 *
 * listener.onRebalance((event) => {
 *   console.log(`Rebalance: ${event.protocol} status=${event.status}`);
 * });
 *
 * await listener.start();
 *
 * // Later:
 * await listener.stop();
 * ```
 */

import * as StellarSdk from '@stellar/stellar-sdk';

import type {
  DepositEvent,
  WithdrawEvent,
  RebalanceEvent,
  RebalanceFailedEvent,
  ProtocolChangedEvent,
  VaultInitializedEvent,
  VaultPausedEvent,
  VaultUnpausedEvent,
  EmergencyPausedEvent,
  TvlCapUpdatedEvent,
  UserDepositCapUpdatedEvent,
  CapsUpdatedEvent,
  LimitsUpdatedEvent,
  DepositLimitsUpdatedEvent,
  AgentUpdatedEvent,
  AgentUpdateProposedEvent,
  AgentUpdateConfirmedEvent,
  AgentUpdateCancelledEvent,
  OwnershipTransferInitiatedEvent,
  OwnershipTransferredEvent,
  OwnershipTransferCancelledEvent,
  AssetsUpdatedEvent,
  UpgradedEvent,
  UpgradeScheduledEvent,
  UpgradeCancelledEvent,
  BlendSupplyEvent,
  BlendWithdrawEvent,
  BlendPoolConfiguredEvent,
  DexSupplyEvent,
  DexWithdrawEvent,
  DexPoolConfiguredEvent,
  UserStrategyUpdatedEvent,
} from './generated/vault';

// ---------------------------------------------------------------------------
// Event topic constants (must match contracts/vault/src/topics.rs)
// ---------------------------------------------------------------------------

/** Topic symbols emitted by the contract, keyed by event name. */
const EVENT_TOPICS = {
  deposit: 'deposit',
  withdraw: 'withdraw',
  rebalance: 'rebalance',
  rebalance_failed: 'reb_fail',
  protocol_changed: 'proto_chg',
  initialized: 'init',
  paused: 'paused',
  unpaused: 'unpaused',
  emergency_paused: 'emerg',
  tvl_cap_updated: 'tvl_cap',
  user_cap_updated: 'user_cap',
  limits_updated: 'l_upd',
  deposit_limits_updated: 'dep_lim',
  caps_updated: 'caps_upd',
  agent_updated: 'agent',
  agent_update_proposed: 'agt_prop',
  agent_update_confirmed: 'agt_conf',
  agent_update_cancelled: 'agt_cncl',
  ownership_initiated: 'own_init',
  ownership_transferred: 'own_xfer',
  ownership_cancelled: 'own_cncl',
  assets_updated: 'assets',
  upgraded: 'upgraded',
  upgrade_scheduled: 'upg_sched',
  upgrade_cancelled: 'upg_cncl',
  blend_supply: 'blend_sup',
  blend_withdraw: 'blend_wd',
  blend_pool_configured: 'blend_cfg',
  dex_supply: 'dex_sup',
  dex_withdraw: 'dex_wd',
  dex_pool_configured: 'dex_cfg',
  user_strategy_updated: 'usr_strat',
} as const;

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/** Callback type for event handlers. */
export type EventHandler<T> = (event: T, raw: StellarSdk.rpc.Api.EventResponse) => void;

/** Options for creating a VaultEventListener. */
export interface EventListenerOptions {
  /** Deployed vault contract address (C...). */
  contractId: string;
  /** Soroban RPC server instance. */
  server: StellarSdk.rpc.Server;
  /** Network passphrase. */
  networkPassphrase: string;
  /** Start ledger for event streaming (defaults to latest). */
  startLedger?: number;
  /** Maximum number of events to process per batch. */
  batchSize?: number;
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/**
 * Decode a Soroban event response into a typed event payload.
 * The event's `value` is the XDR-encoded ScVal; we decode it with
 * `scValToNative` which returns a JS object matching the struct shape.
 */
function decodeEvent<T>(response: StellarSdk.rpc.Api.EventResponse): T {
  return StellarSdk.scValToNative(response.value) as unknown as T;
}

/**
 * Check whether a Soroban event response has a specific topic as its
 * first (or any) topic entry.
 */
function hasTopic(
  response: StellarSdk.rpc.Api.EventResponse,
  topic: string,
): boolean {
  return response.topic.some((t: any) => {
    const native = StellarSdk.scValToNative(t);
    return native === topic;
  });
}

/**
 * Extract the user address from topic 1 (if present).
 * Some events (deposit, withdraw, user_strategy_updated) publish the
 * user's Address as topic 1.
 */
function extractUserTopic(
  response: StellarSdk.rpc.Api.EventResponse,
): string | undefined {
  if (response.topic.length < 2) return undefined;
  const native = StellarSdk.scValToNative(response.topic[1]);
  return typeof native === 'string' ? native : undefined;
}

// ---------------------------------------------------------------------------
// VaultEventListener
// ---------------------------------------------------------------------------

/**
 * Type-safe event listener for NeuroWealth Vault Soroban events.
 *
 * Wraps Soroban RPC event streaming and provides typed handler methods
 * for each event type. Supports filtering by topic and user address.
 */
export class VaultEventListener {
  private readonly contractId: string;
  private readonly server: StellarSdk.rpc.Server;
  private readonly networkPassphrase: string;
  private readonly batchSize: number;

  private handlers: Map<string, EventHandler<unknown>[]> = new Map();
  private userFilters: Map<string, Set<string>> = new Map();
  private running = false;
  private abortController: AbortController | null = null;

  constructor(options: EventListenerOptions) {
    this.contractId = options.contractId;
    this.server = options.server;
    this.networkPassphrase = options.networkPassphrase;
    this.batchSize = options.batchSize ?? 100;
  }

  // -----------------------------------------------------------------------
  // Handler registration
  // -----------------------------------------------------------------------

  /** Register a handler for deposit events. */
  onDeposit(handler: EventHandler<DepositEvent>, userFilter?: string): this {
    return this.on('deposit', handler, userFilter);
  }

  /** Register a handler for withdraw events. */
  onWithdraw(handler: EventHandler<WithdrawEvent>, userFilter?: string): this {
    return this.on('withdraw', handler, userFilter);
  }

  /** Register a handler for rebalance events. */
  onRebalance(handler: EventHandler<RebalanceEvent>): this {
    return this.on('rebalance', handler);
  }

  /** Register a handler for rebalance failed events. */
  onRebalanceFailed(handler: EventHandler<RebalanceFailedEvent>): this {
    return this.on('rebalance_failed', handler);
  }

  /** Register a handler for protocol changed events. */
  onProtocolChanged(handler: EventHandler<ProtocolChangedEvent>): this {
    return this.on('protocol_changed', handler);
  }

  /** Register a handler for vault initialized events. */
  onInitialized(handler: EventHandler<VaultInitializedEvent>): this {
    return this.on('initialized', handler);
  }

  /** Register a handler for vault paused events. */
  onPaused(handler: EventHandler<VaultPausedEvent>): this {
    return this.on('paused', handler);
  }

  /** Register a handler for vault unpaused events. */
  onUnpaused(handler: EventHandler<VaultUnpausedEvent>): this {
    return this.on('unpaused', handler);
  }

  /** Register a handler for emergency paused events. */
  onEmergencyPaused(handler: EventHandler<EmergencyPausedEvent>): this {
    return this.on('emergency_paused', handler);
  }

  /** Register a handler for TVL cap updated events. */
  onTvlCapUpdated(handler: EventHandler<TvlCapUpdatedEvent>): this {
    return this.on('tvl_cap_updated', handler);
  }

  /** Register a handler for user deposit cap updated events. */
  onUserDepositCapUpdated(handler: EventHandler<UserDepositCapUpdatedEvent>): this {
    return this.on('user_cap_updated', handler);
  }

  /** Register a handler for caps updated events. */
  onCapsUpdated(handler: EventHandler<CapsUpdatedEvent>): this {
    return this.on('caps_updated', handler);
  }

  /** Register a handler for limits updated events. */
  onLimitsUpdated(handler: EventHandler<LimitsUpdatedEvent>): this {
    return this.on('limits_updated', handler);
  }

  /** Register a handler for deposit limits updated events. */
  onDepositLimitsUpdated(handler: EventHandler<DepositLimitsUpdatedEvent>): this {
    return this.on('deposit_limits_updated', handler);
  }

  /** Register a handler for agent updated events. */
  onAgentUpdated(handler: EventHandler<AgentUpdatedEvent>): this {
    return this.on('agent_updated', handler);
  }

  /** Register a handler for agent update proposed events. */
  onAgentUpdateProposed(handler: EventHandler<AgentUpdateProposedEvent>): this {
    return this.on('agent_update_proposed', handler);
  }

  /** Register a handler for agent update confirmed events. */
  onAgentUpdateConfirmed(handler: EventHandler<AgentUpdateConfirmedEvent>): this {
    return this.on('agent_update_confirmed', handler);
  }

  /** Register a handler for agent update cancelled events. */
  onAgentUpdateCancelled(handler: EventHandler<AgentUpdateCancelledEvent>): this {
    return this.on('agent_update_cancelled', handler);
  }

  /** Register a handler for ownership transfer initiated events. */
  onOwnershipInitiated(handler: EventHandler<OwnershipTransferInitiatedEvent>): this {
    return this.on('ownership_initiated', handler);
  }

  /** Register a handler for ownership transferred events. */
  onOwnershipTransferred(handler: EventHandler<OwnershipTransferredEvent>): this {
    return this.on('ownership_transferred', handler);
  }

  /** Register a handler for ownership transfer cancelled events. */
  onOwnershipCancelled(handler: EventHandler<OwnershipTransferCancelledEvent>): this {
    return this.on('ownership_cancelled', handler);
  }

  /** Register a handler for assets updated events. */
  onAssetsUpdated(handler: EventHandler<AssetsUpdatedEvent>): this {
    return this.on('assets_updated', handler);
  }

  /** Register a handler for upgraded events. */
  onUpgraded(handler: EventHandler<UpgradedEvent>): this {
    return this.on('upgraded', handler);
  }

  /** Register a handler for upgrade scheduled events. */
  onUpgradeScheduled(handler: EventHandler<UpgradeScheduledEvent>): this {
    return this.on('upgrade_scheduled', handler);
  }

  /** Register a handler for upgrade cancelled events. */
  onUpgradeCancelled(handler: EventHandler<UpgradeCancelledEvent>): this {
    return this.on('upgrade_cancelled', handler);
  }

  /** Register a handler for Blend supply events. */
  onBlendSupply(handler: EventHandler<BlendSupplyEvent>): this {
    return this.on('blend_supply', handler);
  }

  /** Register a handler for Blend withdraw events. */
  onBlendWithdraw(handler: EventHandler<BlendWithdrawEvent>): this {
    return this.on('blend_withdraw', handler);
  }

  /** Register a handler for Blend pool configured events. */
  onBlendPoolConfigured(handler: EventHandler<BlendPoolConfiguredEvent>): this {
    return this.on('blend_pool_configured', handler);
  }

  /** Register a handler for DEX supply events. */
  onDexSupply(handler: EventHandler<DexSupplyEvent>): this {
    return this.on('dex_supply', handler);
  }

  /** Register a handler for DEX withdraw events. */
  onDexWithdraw(handler: EventHandler<DexWithdrawEvent>): this {
    return this.on('dex_withdraw', handler);
  }

  /** Register a handler for DEX pool configured events. */
  onDexPoolConfigured(handler: EventHandler<DexPoolConfiguredEvent>): this {
    return this.on('dex_pool_configured', handler);
  }

  /** Register a handler for user strategy updated events. */
  onUserStrategyUpdated(handler: EventHandler<UserStrategyUpdatedEvent>, userFilter?: string): this {
    return this.on('user_strategy_updated', handler, userFilter);
  }

  // -----------------------------------------------------------------------
  // Generic handler registration
  // -----------------------------------------------------------------------

  /**
   * Register a handler for a specific event topic.
   *
   * @param topicKey  Key into `EVENT_TOPICS` (e.g., "deposit", "rebalance").
   * @param handler   Callback invoked with the decoded event payload.
   * @param userFilter  If provided, only events with this user address as
   *                    topic 1 will be forwarded to the handler.
   */
  on<T>(topicKey: string, handler: EventHandler<T>, userFilter?: string): this {
    if (!this.handlers.has(topicKey)) {
      this.handlers.set(topicKey, []);
    }
    this.handlers.get(topicKey)!.push(handler as EventHandler<unknown>);

    if (userFilter) {
      if (!this.userFilters.has(topicKey)) {
        this.userFilters.set(topicKey, new Set());
      }
      this.userFilters.get(topicKey)!.add(userFilter);
    }

    return this;
  }

  // -----------------------------------------------------------------------
  // Lifecycle
  // -----------------------------------------------------------------------

  /**
   * Start listening for events. Uses Soroban's `getEvents` polling loop.
   * Call `stop()` to terminate.
   *
   * @param startLedger  Ledger to start from. Defaults to the latest ledger.
   */
  async start(startLedger?: number): Promise<void> {
    if (this.running) return;
    this.running = true;
    this.abortController = new AbortController();

    const initialLedger = startLedger ?? (await this.getLatestLedger());
    let cursor: string | undefined;

    while (this.running) {
      try {
        const request: StellarSdk.rpc.Server.GetEventsRequest = {
          limit: this.batchSize,
          filters: [
            {
              type: 'contract',
              contractIds: [this.contractId],
            },
          ],
          ...(cursor ? { cursor } : { startLedger: initialLedger }),
        };
        const events = await this.server.getEvents(request);

        for (const event of events.events) {
          this.dispatchEvent(event);
        }

        if (events.cursor) {
          cursor = events.cursor;
        } else if (events.events.length > 0) {
          const lastEvent = events.events[events.events.length - 1];
          cursor = (lastEvent as any)?.pagingToken ?? lastEvent?.id;
        }

        // If we got fewer events than the batch size, wait before polling again
        if (events.events.length < this.batchSize) {
          await this.sleep(5000);
        }
      } catch (err) {
        if (!this.running) break;
        console.error('VaultEventListener error:', err);
        await this.sleep(10_000);
      }
    }
  }

  /** Stop listening for events. */
  stop(): void {
    this.running = false;
    this.abortController?.abort();
    this.abortController = null;
  }

  /** Check whether the listener is currently running. */
  get isRunning(): boolean {
    return this.running;
  }

  // -----------------------------------------------------------------------
  // Internal
  // -----------------------------------------------------------------------

  private dispatchEvent(event: StellarSdk.rpc.Api.EventResponse): void {
    if (event.topic.length === 0) return;

    const primaryTopic = StellarSdk.scValToNative(event.topic[0]);
    if (typeof primaryTopic !== 'string') return;

    // Find which topic key matches
    let matchedKey: string | undefined;
    for (const [key, symbol] of Object.entries(EVENT_TOPICS)) {
      if (symbol === primaryTopic) {
        matchedKey = key;
        break;
      }
    }

    if (!matchedKey) return;

    const handlers = this.handlers.get(matchedKey);
    if (!handlers || handlers.length === 0) return;

    // Apply user filter if configured
    const userFilterSet = this.userFilters.get(matchedKey);
    if (userFilterSet && userFilterSet.size > 0) {
      const userAddr = extractUserTopic(event);
      if (!userAddr || !userFilterSet.has(userAddr)) return;
    }

    // Decode and dispatch to all registered handlers
    const payload = decodeEvent<unknown>(event);
    for (const handler of handlers) {
      try {
        handler(payload, event);
      } catch (err) {
        console.error(`VaultEventListener handler error (${matchedKey}):`, err);
      }
    }
  }

  private async getLatestLedger(): Promise<number> {
    const response = await this.server.getLatestLedger();
    return Number(response.sequence);
  }

  private sleep(ms: number): Promise<void> {
    return new Promise((resolve) => setTimeout(resolve, ms));
  }
}
