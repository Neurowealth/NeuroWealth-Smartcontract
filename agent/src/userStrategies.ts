/**
 * Per-user strategy lookup from Postgres (Issue #749).
 *
 * The rebalance loop and the deposit handler previously evaluated every user
 * as `balanced` against a hardcoded `blend @ 6.5%` position. They now read
 * `users.strategy_preference` and the vault's current allocation from the
 * latest `rebalances` row (see db/schema.sql).
 */

import logger from './logger';

export type Strategy = 'conservative' | 'balanced' | 'growth';

export const STRATEGIES: readonly Strategy[] = ['conservative', 'balanced', 'growth'];
export const DEFAULT_STRATEGY: Strategy = 'balanced';

export interface UserStrategy {
  userId: string;
  stellarAddress: string;
  strategy: Strategy;
}

export interface CurrentAllocation {
  protocol: string;
  apy: number;
}

export interface RebalanceDecision {
  shouldRebalance: boolean;
  targetProtocol?: string;
}

export type EvaluateYield = (
  strategy: string,
  currentProtocol: string,
  currentApy: number,
) => Promise<RebalanceDecision>;

type Queryable = { query: (sql: string, values?: unknown[]) => Promise<unknown> };
type Rows<T> = { rows: T[] };

export function normalizeStrategy(value: unknown): Strategy {
  return STRATEGIES.includes(value as Strategy) ? (value as Strategy) : DEFAULT_STRATEGY;
}

export async function listUserStrategies(pool: Queryable): Promise<UserStrategy[]> {
  const result = (await pool.query(
    'SELECT id, stellar_address, strategy_preference FROM users ORDER BY created_at',
  )) as Rows<{ id: string; stellar_address: string; strategy_preference: string }>;
  return result.rows.map((row) => ({
    userId: row.id,
    stellarAddress: row.stellar_address,
    strategy: normalizeStrategy(row.strategy_preference),
  }));
}

/** Strategy for one Stellar address; falls back to the default when unknown. */
export async function getUserStrategy(pool: Queryable, stellarAddress: string): Promise<Strategy> {
  const result = (await pool.query(
    'SELECT strategy_preference FROM users WHERE stellar_address = $1',
    [stellarAddress],
  )) as Rows<{ strategy_preference: string }>;
  if (result.rows.length === 0) {
    logger.warn({ stellarAddress }, 'User not found in database; using default strategy');
    return DEFAULT_STRATEGY;
  }
  return normalizeStrategy(result.rows[0].strategy_preference);
}

/** Vault's current protocol and APY from the most recent rebalance. */
export async function getCurrentAllocation(pool: Queryable): Promise<CurrentAllocation> {
  const result = (await pool.query(
    'SELECT protocol, apy_after FROM rebalances ORDER BY timestamp DESC LIMIT 1',
  )) as Rows<{ protocol: string; apy_after: string | number }>;
  if (result.rows.length === 0) return { protocol: 'none', apy: 0 };
  const apy = Number(result.rows[0].apy_after);
  return { protocol: result.rows[0].protocol, apy: Number.isFinite(apy) ? apy : 0 };
}

export interface RebalanceCycleResult {
  usersEvaluated: number;
  decisions: Map<Strategy, RebalanceDecision>;
  currentAllocation: CurrentAllocation;
  strategySnapshot: Array<{ userId: string; strategy: Strategy }>;
}

/**
 * Evaluates yield for every user's stored strategy. Users sharing a strategy
 * share one evaluation, since the inputs are identical.
 */
export async function runRebalanceCycle(
  pool: Queryable,
  evaluate: EvaluateYield,
): Promise<RebalanceCycleResult> {
  const [users, current] = await Promise.all([listUserStrategies(pool), getCurrentAllocation(pool)]);
  const decisions = new Map<Strategy, RebalanceDecision>();

  for (const user of users) {
    let decision = decisions.get(user.strategy);
    if (!decision) {
      decision = await evaluate(user.strategy, current.protocol, current.apy);
      decisions.set(user.strategy, decision);
    }
    if (decision.shouldRebalance) {
      logger.info(
        { userId: user.userId, strategy: user.strategy, from: current.protocol, targetProtocol: decision.targetProtocol },
        'Rebalance needed for user',
      );
    }
  }

  return {
    usersEvaluated: users.length,
    decisions,
    currentAllocation: current,
    strategySnapshot: users.map(({ userId, strategy }) => ({ userId, strategy })),
  };
}
