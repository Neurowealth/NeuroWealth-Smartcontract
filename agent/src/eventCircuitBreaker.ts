import logger from './logger';

export type CircuitBreakerState = 'CLOSED' | 'OPEN' | 'HALF_OPEN';

export interface EventCircuitBreakerConfig {
  /** Number of consecutive failures before the circuit breaker trips to OPEN. Default: 5 */
  failureThreshold?: number;
  /** Cooldown time in ms before transitioning from OPEN to HALF_OPEN to attempt a probe. Default: 30000 */
  cooldownMs?: number;
  /** Base polling interval in ms under normal conditions. Default: 5000 */
  baseDelayMs?: number;
  /** Maximum exponential backoff delay in ms. Default: 60000 */
  maxDelayMs?: number;
  /** Exponential backoff multiplier. Default: 2 */
  backoffMultiplier?: number;
  /** Whether to add random jitter (0-500ms) to backoff intervals. Default: true */
  jitter?: boolean;
  /** Optional custom time source for deterministic testing. */
  now?: () => number;
  /** Callback fired on state transitions. */
  onStateChange?: (from: CircuitBreakerState, to: CircuitBreakerState, reason?: string) => void;
  /** Callback fired when circuit trips to OPEN. */
  onTrip?: (consecutiveFailures: number, lastError?: string) => void;
}

export interface CircuitBreakerStatus {
  state: CircuitBreakerState;
  consecutiveFailures: number;
  failureThreshold: number;
  lastFailureTime: number | null;
  lastSuccessTime: number | null;
  lastError: string | null;
  nextProbeTime: number | null;
  cooldownMs: number;
  baseDelayMs: number;
  maxDelayMs: number;
}

export class EventCircuitBreaker {
  private state: CircuitBreakerState = 'CLOSED';
  private consecutiveFailures = 0;
  private lastFailureTime: number | null = null;
  private lastSuccessTime: number | null = null;
  private lastError: string | null = null;
  private nextProbeTime: number | null = null;

  readonly failureThreshold: number;
  readonly cooldownMs: number;
  readonly baseDelayMs: number;
  readonly maxDelayMs: number;
  readonly backoffMultiplier: number;
  readonly jitter: boolean;
  private readonly now: () => number;
  private readonly onStateChange?: (from: CircuitBreakerState, to: CircuitBreakerState, reason?: string) => void;
  private readonly onTrip?: (consecutiveFailures: number, lastError?: string) => void;

  constructor(config: EventCircuitBreakerConfig = {}) {
    this.failureThreshold = config.failureThreshold ?? parseInt(process.env.SOROBAN_CIRCUIT_BREAKER_THRESHOLD || '5', 10);
    this.cooldownMs = config.cooldownMs ?? parseInt(process.env.SOROBAN_CIRCUIT_BREAKER_COOLDOWN_MS || '30000', 10);
    this.baseDelayMs = config.baseDelayMs ?? parseInt(process.env.SOROBAN_POLL_INTERVAL_MS || '5000', 10);
    this.maxDelayMs = config.maxDelayMs ?? parseInt(process.env.SOROBAN_POLL_MAX_BACKOFF_MS || '60000', 10);
    this.backoffMultiplier = config.backoffMultiplier ?? 2;
    this.jitter = config.jitter ?? true;
    this.now = config.now ?? Date.now;
    this.onStateChange = config.onStateChange;
    this.onTrip = config.onTrip;
  }

  /**
   * Returns current circuit breaker state, automatically promoting
   * OPEN to HALF_OPEN once the cooldown time has elapsed.
   */
  getState(): CircuitBreakerState {
    if (this.state === 'OPEN' && this.nextProbeTime !== null && this.now() >= this.nextProbeTime) {
      this.transitionTo('HALF_OPEN', 'Cooldown elapsed; probing RPC health');
    }
    return this.state;
  }

  /**
   * Determines whether a poll cycle should be permitted.
   */
  canExecute(): boolean {
    const currentState = this.getState();
    return currentState === 'CLOSED' || currentState === 'HALF_OPEN';
  }

  /**
   * Records a successful event polling cycle.
   * Resets failures and restores CLOSED state if previously OPEN or HALF_OPEN.
   */
  recordSuccess(): void {
    const previousState = this.state;
    this.consecutiveFailures = 0;
    this.lastSuccessTime = this.now();
    this.lastError = null;
    this.nextProbeTime = null;

    if (previousState !== 'CLOSED') {
      this.transitionTo('CLOSED', 'Successful event poll; circuit recovered');
      logger.info('Soroban event polling circuit breaker closed after successful probe');
    }
  }

  /**
   * Records a failed event polling cycle.
   * Increments consecutive failure counter and applies exponential backoff or trips breaker.
   */
  recordFailure(error?: unknown): void {
    const previousState = this.getState();
    this.consecutiveFailures++;
    this.lastFailureTime = this.now();
    this.lastError = error instanceof Error ? error.message : String(error ?? 'Unknown error');

    if (previousState === 'HALF_OPEN') {
      // Probe failed: trip immediately back to OPEN with backoff
      const backoffDelay = this.calculateBackoffDelay();
      this.nextProbeTime = this.now() + Math.max(this.cooldownMs, backoffDelay);
      this.transitionTo('OPEN', `Probe failed: ${this.lastError}`);
      this.emitTrip();
      return;
    }

    if (this.consecutiveFailures >= this.failureThreshold) {
      if (previousState === 'CLOSED') {
        const backoffDelay = this.calculateBackoffDelay();
        this.nextProbeTime = this.now() + Math.max(this.cooldownMs, backoffDelay);
        this.transitionTo('OPEN', `Threshold reached (${this.consecutiveFailures}/${this.failureThreshold} failures): ${this.lastError}`);
        this.emitTrip();
      }
    }
  }

  /**
   * Computes the recommended delay until the next poll cycle based on
   * current circuit state and consecutive failure count.
   */
  getNextDelayMs(): number {
    const currentState = this.getState();

    if (currentState === 'OPEN') {
      if (this.nextProbeTime === null) {
        return this.cooldownMs;
      }
      return Math.max(100, this.nextProbeTime - this.now());
    }

    if (currentState === 'HALF_OPEN') {
      return this.baseDelayMs;
    }

    // CLOSED state:
    if (this.consecutiveFailures === 0) {
      return this.baseDelayMs;
    }

    return this.calculateBackoffDelay();
  }

  /**
   * Calculates exponential backoff with optional jitter.
   */
  calculateBackoffDelay(): number {
    const exponent = Math.max(0, this.consecutiveFailures - 1);
    const exponential = this.baseDelayMs * Math.pow(this.backoffMultiplier, exponent);
    const capped = Math.min(exponential, this.maxDelayMs);
    const jitterAmount = this.jitter ? Math.floor(Math.random() * 500) : 0;
    return Math.min(capped + jitterAmount, this.maxDelayMs);
  }

  /**
   * Manually resets the circuit breaker to CLOSED state.
   */
  reset(): void {
    const previousState = this.state;
    this.consecutiveFailures = 0;
    this.lastError = null;
    this.nextProbeTime = null;
    this.transitionTo('CLOSED', 'Manual reset');
    if (previousState !== 'CLOSED') {
      logger.info('Soroban event polling circuit breaker manually reset to CLOSED');
    }
  }

  /**
   * Manually trips the circuit breaker to OPEN state.
   */
  trip(reason = 'Manual trip'): void {
    this.nextProbeTime = this.now() + this.cooldownMs;
    this.transitionTo('OPEN', reason);
    this.emitTrip();
  }

  getStatus(): CircuitBreakerStatus {
    return {
      state: this.getState(),
      consecutiveFailures: this.consecutiveFailures,
      failureThreshold: this.failureThreshold,
      lastFailureTime: this.lastFailureTime,
      lastSuccessTime: this.lastSuccessTime,
      lastError: this.lastError,
      nextProbeTime: this.nextProbeTime,
      cooldownMs: this.cooldownMs,
      baseDelayMs: this.baseDelayMs,
      maxDelayMs: this.maxDelayMs,
    };
  }

  private transitionTo(to: CircuitBreakerState, reason?: string): void {
    if (this.state === to) return;
    const from = this.state;
    this.state = to;
    logger.warn({ from, to, reason, consecutiveFailures: this.consecutiveFailures }, 'Soroban polling circuit breaker state transition');
    if (this.onStateChange) {
      this.onStateChange(from, to, reason);
    }
  }

  private emitTrip(): void {
    logger.error(
      {
        consecutiveFailures: this.consecutiveFailures,
        threshold: this.failureThreshold,
        lastError: this.lastError,
        nextProbeAt: this.nextProbeTime ? new Date(this.nextProbeTime).toISOString() : null,
      },
      'Soroban event polling circuit breaker tripped: RPC is experiencing sustained failures; polling paused/backed off',
    );
    if (this.onTrip) {
      this.onTrip(this.consecutiveFailures, this.lastError ?? undefined);
    }
  }
}
