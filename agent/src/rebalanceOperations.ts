import { createHash } from 'node:crypto';
import type { ExecutionMode } from './executionMode';

export type RebalanceOperationStatus =
  | 'prepared'
  | 'submitting'
  | 'submitted'
  | 'reconciling'
  | 'confirmed'
  | 'rejected'
  | 'dry_run';

export interface RebalanceOperation {
  operation_id: string;
  decision_id: string;
  attempt: number;
  status: RebalanceOperationStatus;
  tx_hash: string | null;
  retry_reason: string | null;
  last_error: string | null;
}

export interface OperationReservation {
  operation: RebalanceOperation;
  created: boolean;
}

export interface RebalanceOperationRepository {
  reserve(input: {
    decisionId: string;
    snapshot: unknown;
    retryReason?: string;
  }): Promise<OperationReservation>;
  recordDryRun(input: {
    decisionId: string;
    snapshot: unknown;
    simulation: unknown;
    rejectionReasons: string[];
  }): Promise<void>;
  update(
    operationId: string,
    status: RebalanceOperationStatus,
    fields?: { txHash?: string; error?: string | null; simulation?: unknown },
  ): Promise<void>;
}

interface PgClientLike {
  query<T = Record<string, unknown>>(sql: string, values?: unknown[]): Promise<{ rows: T[] }>;
  release(): void;
}

interface PgPoolLike {
  connect(): Promise<PgClientLike>;
  query<T = Record<string, unknown>>(sql: string, values?: unknown[]): Promise<{ rows: T[] }>;
}

function stableJson(value: unknown): string {
  if (Array.isArray(value)) return `[${value.map(stableJson).join(',')}]`;
  if (value !== null && typeof value === 'object') {
    const record = value as Record<string, unknown>;
    return `{${Object.keys(record).sort().map((key) => `${JSON.stringify(key)}:${stableJson(record[key])}`).join(',')}}`;
  }
  return JSON.stringify(value) ?? 'null';
}

export function createDecisionId(source: string, cycleKey: string, snapshot: unknown): string {
  return createHash('sha256')
    .update(`${source}\0${cycleKey}\0${stableJson(snapshot)}`)
    .digest('hex');
}

function operationIdForRetry(decisionId: string, attempt: number, reason: string): string {
  return createHash('sha256').update(`${decisionId}\0${attempt}\0${reason}`).digest('hex');
}

export class PostgresRebalanceOperationRepository implements RebalanceOperationRepository {
  constructor(private readonly pool: PgPoolLike) {}

  async reserve(input: {
    decisionId: string;
    snapshot: unknown;
    retryReason?: string;
  }): Promise<OperationReservation> {
    const client = await this.pool.connect();
    try {
      await client.query('BEGIN');
      await client.query('SELECT pg_advisory_xact_lock(hashtext($1))', [input.decisionId]);
      const latestResult = await client.query<RebalanceOperation>(
        `SELECT operation_id, decision_id, attempt, status, tx_hash, retry_reason, last_error
         FROM agent_rebalance_operations WHERE decision_id = $1 AND execution_mode = 'live'
         ORDER BY attempt DESC LIMIT 1 FOR UPDATE`,
        [input.decisionId],
      );
      const latest = latestResult.rows[0];

      if (latest && (latest.status !== 'rejected' || !input.retryReason)) {
        await client.query('COMMIT');
        return { operation: latest, created: false };
      }
      if (latest && input.retryReason?.trim() === '') {
        throw new Error('A retry reason must not be blank');
      }
      if (!latest && input.retryReason) {
        throw new Error('A retry is allowed only after a confirmed rejection');
      }

      const attempt = (latest?.attempt ?? 0) + 1;
      const operationId = latest
        ? operationIdForRetry(input.decisionId, attempt, input.retryReason!)
        : input.decisionId;
      const inserted = await client.query<RebalanceOperation>(
        `INSERT INTO agent_rebalance_operations
           (operation_id, decision_id, attempt, execution_mode, status, input_snapshot, retry_reason)
         VALUES ($1, $2, $3, 'live', 'prepared', $4::jsonb, $5)
         RETURNING operation_id, decision_id, attempt, status, tx_hash, retry_reason, last_error`,
        [operationId, input.decisionId, attempt, JSON.stringify(input.snapshot), input.retryReason ?? null],
      );
      await client.query('COMMIT');
      return { operation: inserted.rows[0], created: true };
    } catch (error) {
      await client.query('ROLLBACK');
      throw error;
    } finally {
      client.release();
    }
  }

  async recordDryRun(input: {
    decisionId: string;
    snapshot: unknown;
    simulation: unknown;
    rejectionReasons: string[];
  }): Promise<void> {
    if (!process.env.DATABASE_URL) return;
    const operationId = `dry-run:${input.decisionId}`;
    await this.pool.query(
      `INSERT INTO agent_rebalance_operations
         (operation_id, decision_id, attempt, execution_mode, status, input_snapshot, simulation_result, last_error)
       VALUES ($1, $2, 1, 'dry-run', 'dry_run', $3::jsonb, $4::jsonb, $5)
       ON CONFLICT (operation_id) DO UPDATE SET
         input_snapshot = EXCLUDED.input_snapshot,
         simulation_result = EXCLUDED.simulation_result,
         last_error = EXCLUDED.last_error,
         updated_at = NOW()`,
      [
        operationId,
        input.decisionId,
        JSON.stringify(input.snapshot),
        JSON.stringify(input.simulation),
        input.rejectionReasons.join('; ') || null,
      ],
    );
  }

  async update(
    operationId: string,
    status: RebalanceOperationStatus,
    fields: { txHash?: string; error?: string | null; simulation?: unknown } = {},
  ): Promise<void> {
    await this.pool.query(
      `UPDATE agent_rebalance_operations SET
         status = $2,
         tx_hash = COALESCE($3, tx_hash),
         last_error = $4,
         simulation_result = COALESCE($5::jsonb, simulation_result),
         updated_at = NOW()
       WHERE operation_id = $1`,
      [
        operationId,
        status,
        fields.txHash ?? null,
        fields.error ?? null,
        fields.simulation === undefined ? null : JSON.stringify(fields.simulation),
      ],
    );
  }
}

export interface OperationSimulation {
  success: boolean;
  result?: unknown;
  rejectionReasons?: string[];
}

export interface PreparedOperation {
  txHash: string;
  send(): Promise<{ accepted: boolean; error?: string }>;
}

export interface OperationTransport {
  simulate(): Promise<OperationSimulation>;
  prepare(): Promise<PreparedOperation>;
  reconcile(txHash: string): Promise<'confirmed' | 'rejected' | 'unknown'>;
}

export interface OperationResult {
  operationId: string;
  status: RebalanceOperationStatus;
  txHash?: string;
  simulation?: unknown;
  rejectionReasons: string[];
}

function toResult(operation: RebalanceOperation, rejectionReasons: string[] = []): OperationResult {
  return {
    operationId: operation.operation_id,
    status: operation.status,
    ...(operation.tx_hash ? { txHash: operation.tx_hash } : {}),
    rejectionReasons: operation.last_error && ['rejected', 'reconciling'].includes(operation.status)
      ? [operation.last_error]
      : rejectionReasons,
  };
}

export async function executeRebalanceOperation(input: {
  mode: ExecutionMode;
  decisionId: string;
  snapshot: unknown;
  retryReason?: string;
  repository: RebalanceOperationRepository;
  transport: OperationTransport;
}): Promise<OperationResult> {
  if (input.mode === 'dry-run') {
    const simulation = await input.transport.simulate();
    const rejectionReasons = simulation.rejectionReasons ?? [];
    await input.repository.recordDryRun({
      decisionId: input.decisionId,
      snapshot: input.snapshot,
      simulation: simulation.result,
      rejectionReasons,
    });
    return {
      operationId: `dry-run:${input.decisionId}`,
      status: 'dry_run',
      simulation: simulation.result,
      rejectionReasons,
    };
  }

  const reservation = await input.repository.reserve({
    decisionId: input.decisionId,
    snapshot: input.snapshot,
    retryReason: input.retryReason,
  });
  const operation = reservation.operation;
  if (!reservation.created) {
    if (!operation.tx_hash || !['submitting', 'submitted', 'reconciling'].includes(operation.status)) {
      return toResult(operation);
    }
    const reconciled = await input.transport.reconcile(operation.tx_hash);
    const status = reconciled === 'confirmed'
      ? 'confirmed'
      : reconciled === 'rejected'
        ? 'rejected'
        : 'reconciling';
    await input.repository.update(operation.operation_id, status);
    return { ...toResult({ ...operation, status }), txHash: operation.tx_hash };
  }

  let simulation: OperationSimulation;
  try {
    simulation = await input.transport.simulate();
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    await input.repository.update(operation.operation_id, 'rejected', { error: reason });
    return { ...toResult({ ...operation, status: 'rejected', last_error: reason }), rejectionReasons: [reason] };
  }
  if (!simulation.success) {
    const reason = simulation.rejectionReasons?.join('; ') || 'Transaction simulation rejected';
    await input.repository.update(operation.operation_id, 'rejected', {
      error: reason,
      simulation: simulation.result,
    });
    return {
      ...toResult({ ...operation, status: 'rejected', last_error: reason }),
      simulation: simulation.result,
      rejectionReasons: simulation.rejectionReasons ?? [reason],
    };
  }

  let prepared: PreparedOperation;
  try {
    prepared = await input.transport.prepare();
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    await input.repository.update(operation.operation_id, 'rejected', { error: reason });
    return { ...toResult({ ...operation, status: 'rejected', last_error: reason }), rejectionReasons: [reason] };
  }

  await input.repository.update(operation.operation_id, 'submitting', {
    txHash: prepared.txHash,
    simulation: simulation.result,
  });
  try {
    const submitted = await prepared.send();
    const status = submitted.accepted ? 'submitted' : 'rejected';
    await input.repository.update(operation.operation_id, status, { error: submitted.error ?? null });
    return {
      operationId: operation.operation_id,
      status,
      txHash: prepared.txHash,
      simulation: simulation.result,
      rejectionReasons: submitted.error ? [submitted.error] : [],
    };
  } catch (error) {
    const reason = error instanceof Error ? error.message : String(error);
    await input.repository.update(operation.operation_id, 'reconciling', { error: reason });
    return {
      operationId: operation.operation_id,
      status: 'reconciling',
      txHash: prepared.txHash,
      simulation: simulation.result,
      rejectionReasons: [reason],
    };
  }
}
