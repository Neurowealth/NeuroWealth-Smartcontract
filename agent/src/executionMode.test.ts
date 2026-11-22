import { describe, it } from 'node:test';
import assert from 'node:assert/strict';
import { resolveExecutionMode } from './executionMode';

describe('agent execution mode (#860)', () => {
  it('accepts only explicit live and dry-run modes', () => {
    assert.strictEqual(resolveExecutionMode({ AGENT_EXECUTION_MODE: 'live' }), 'live');
    assert.strictEqual(resolveExecutionMode({ AGENT_EXECUTION_MODE: 'dry-run' }), 'dry-run');
  });

  it('rejects missing, invalid, and mixed legacy mode configuration', () => {
    assert.throws(() => resolveExecutionMode({}), /must be explicitly set/);
    assert.throws(() => resolveExecutionMode({ AGENT_EXECUTION_MODE: 'maybe' }), /must be explicitly set/);
    assert.throws(() => resolveExecutionMode({ AGENT_EXECUTION_MODE: 'live', DRY_RUN: 'true' }), /ambiguous/);
  });
});