import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import {
  createDecisionId,
  executeRebalanceOperation,
  type OperationReservation,
  type OperationResult,
  type OperationTransport,
  type RebalanceOperation,
  type RebalanceOperationRepository,
  type RebalanceOperationStatus,
} from './rebalanceOperations';

class MemoryRepository implements RebalanceOperationRepository {
  readonly operations = new Map<string, RebalanceOperation>();
  readonly dryRuns: Array<{ decisionId: string; snapshot: unknown; simulation: unknown; rejectionReasons: string[] }> = [];

  async reserve(input: { decisionId: string; snapshot: unknown; retryReason?: string }): Promise<OperationReservation> {
    const previous = [...this.operations.values()]
      .filter((operation) => operation.decision_id === input.decisionId)
      .sort((a, b) => b.attempt - a.attempt)[0];
    if (previous && (previous.status !== 'rejected' || !input.retryReason)) {
      return { operation: previous, created: false };
    }
    if (!previous && input.retryReason) throw new Error('A retry is allowed only after a confirmed rejection');

    const attempt = (previous?.attempt ?? 0) + 1;
    const operation: RebalanceOperation = {
      operation_id: previous ? `${input.decisionId}:${attempt}` : input.decisionId,
      decision_id: input.decisionId,
      attempt,
      status: 'prepared',
      tx_hash: null,
      retry_reason: input.retryReason ?? null,
      last_error: null,
    };
    this.operations.set(operation.operation_id, operation);
    return { operation, created: true };
  }

  async recordDryRun(input: { decisionId: string; snapshot: unknown; simulation: unknown; rejectionReasons: string[] }) {
    this.dryRuns.push(input);
  }

  async update(
    operationId: string,
    status: RebalanceOperationStatus,
    fields: { txHash?: string; error?: string | null } = {},
  ) {
    const operation = this.operations.get(operationId);
    assert.ok(operation);
    operation.status = status;
    if (fields.txHash) operation.tx_hash = fields.txHash;
    if (fields.error !== undefined) operation.last_error = fields.error;
  }
}

function fakeTransport(options: {
  send?: () => Promise<{ accepted: boolean; error?: string }>;
  reconcile?: () => Promise<'confirmed' | 'rejected' | 'unknown'>;
} = {}) {
  let sends = 0;
  let simulations = 0;
  let preparations = 0;
  const transport: OperationTransport = {
    async simulate() {
      simulations += 1;
      return { success: true, result: { valid: true } };
    },
    async prepare() {
      preparations += 1;
      return {
        txHash: 'tx-hash-1',
        async send() {
          sends += 1;
          return options.send ? options.send() : { accepted: true };
        },
      };
    },
    async reconcile() {
      return options.reconcile ? options.reconcile() : 'unknown';
    },
  };
  return { transport, get sends() { return sends; }, get simulations() { return simulations; }, get preparations() { return preparations; } };
}

const snapshot = { current: { protocol: 'dex', apy: 7.25 }, target: 'blend' };

async function run(
  repository: MemoryRepository,
  transport: OperationTransport,
  options: { decisionId?: string; mode?: 'live' | 'dry-run'; retryReason?: string } = {},
): Promise<OperationResult> {
  return executeRebalanceOperation({
    mode: options.mode ?? 'live',
    decisionId: options.decisionId ?? 'decision-1',
    snapshot,
    retryReason: options.retryReason,
    repository,
    transport,
  });
}

describe('rebalance operation idempotency (#861)', () => {
  it('derives the same ID for equivalent ordered snapshots', () => {
    assert.strictEqual(
      createDecisionId('hourly', '2026-09-27T10', { b: 2, a: 1 }),
      createDecisionId('hourly', '2026-09-27T10', { a: 1, b: 2 }),
    );
  });

  it('reconciles a timeout after network acceptance without a second send', async () => {
    const repository = new MemoryRepository();
    let accepted = false;
    const tx = fakeTransport({
      async send() {
        accepted = true;
        throw new Error('response timed out');
      },
      async reconcile() {
        return accepted ? 'confirmed' : 'unknown';
      },
    });

    const first = await run(repository, tx.transport);
    const recovered = await run(repository, tx.transport);

    assert.strictEqual(first.status, 'reconciling');
    assert.strictEqual(recovered.status, 'confirmed');
    assert.strictEqual(recovered.operationId, first.operationId);
    assert.strictEqual(tx.sends, 1);
    assert.strictEqual(repository.operations.get(first.operationId)?.decision_id, 'decision-1');
  });

  it('keeps a timeout before network acceptance in reconciliation without retrying blindly', async () => {
    const repository = new MemoryRepository();
    const tx = fakeTransport({
      async send() { throw new Error('connection timed out before response'); },
      async reconcile() { return 'unknown'; },
    });

    const first = await run(repository, tx.transport);
    const repeated = await run(repository, tx.transport);

    assert.strictEqual(first.status, 'reconciling');
    assert.strictEqual(repeated.status, 'reconciling');
    assert.strictEqual(tx.sends, 1);
  });

  it('allows a retry only after confirmed rejection and records its reason', async () => {
    const repository = new MemoryRepository();
    const tx = fakeTransport({ send: async () => ({ accepted: false, error: 'transaction rejected' }) });

    const rejected = await run(repository, tx.transport);
    const blocked = await run(repository, tx.transport);
    const retried = await run(repository, tx.transport, { retryReason: 'RPC confirmed rejection; retry after fee refresh' });

    assert.strictEqual(rejected.status, 'rejected');
    assert.strictEqual(blocked.status, 'rejected');
    assert.notStrictEqual(retried.operationId, rejected.operationId);
    assert.strictEqual(tx.sends, 2);
    assert.strictEqual(repository.operations.get(retried.operationId)?.retry_reason, 'RPC confirmed rejection; retry after fee refresh');
  });

  it('simulates dry-run decisions but never prepares or sends a transaction', async () => {
    const repository = new MemoryRepository();
    const tx = fakeTransport();
    const result = await run(repository, tx.transport, { mode: 'dry-run' });

    assert.strictEqual(result.status, 'dry_run');
    assert.deepStrictEqual(result.simulation, { valid: true });
    assert.strictEqual(tx.simulations, 1);
    assert.strictEqual(tx.preparations, 0);
    assert.strictEqual(tx.sends, 0);
    assert.deepStrictEqual(repository.dryRuns[0].snapshot, snapshot);
  });
});