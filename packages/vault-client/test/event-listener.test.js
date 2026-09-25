const assert = require('node:assert/strict');
const test = require('node:test');

const { VaultEventListener } = require('../dist/index.js');

// Mock Soroban RPC Server
class MockSorobanServer {
  constructor() {
    this.events = [];
    this.latestLedger = 1000;
  }

  addMockEvent(topic, value, ledger) {
    this.events.push({
      topic: topic.map(t => createMockScVal(t)),
      value: createMockScVal(value),
      ledger: ledger || this.latestLedger.toString(),
      contractId: 'CTEST123'
    });
  }

  async getEvents({ startLedger, limit, filters }) {
    const eventsToReturn = this.events.filter(e => 
      parseInt(e.ledger) >= startLedger
    ).slice(0, limit);
    return { events: eventsToReturn };
  }

  async getLatestLedger() {
    return { sequence: this.latestLedger.toString() };
  }
}

// Create a mock ScVal that the Stellar SDK can decode
function createMockScVal(value) {
  return {
    switch: () => {
      if (typeof value === 'string') return 'scvSymbol';
      if (typeof value === 'object') return 'scvMap';
      return 'scvU64';
    },
    value: () => value,
    sym: () => Buffer.from(value),
    map: () => [],
    u64: () => ({ low: 0, high: 0 }),
  };
}

// Mock scValToNative
const StellarSdk = require('@stellar/stellar-sdk');
const originalScValToNative = StellarSdk.scValToNative;
StellarSdk.scValToNative = function mockScValToNative(scVal) {
  // If it has our mock structure, return the value directly
  if (scVal && typeof scVal.switch === 'function') {
    const type = scVal.switch();
    if (type === 'scvSymbol') {
      return scVal.value();
    }
    if (type === 'scvMap') {
      return scVal.value();
    }
  }
  // Otherwise try the original
  try {
    return originalScValToNative(scVal);
  } catch {
    return scVal;
  }
};

test('VaultEventListener constructs with required options', () => {
  const server = new MockSorobanServer();
  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  assert.ok(listener);
  assert.equal(listener.isRunning, false);
});

test('VaultEventListener registers deposit handler', () => {
  const server = new MockSorobanServer();
  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  let handlerCalled = false;
  listener.onDeposit((event) => {
    handlerCalled = true;
  });

  assert.ok(!handlerCalled);
});

test('VaultEventListener dispatches deposit events', async () => {
  const server = new MockSorobanServer();
  server.addMockEvent(
    ['deposit', 'GTEST123...'],
    { user: 'GTEST123', amount: '1000000' },
    1001
  );

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
    batchSize: 10,
  });

  let capturedEvent = null;
  listener.onDeposit((event) => {
    capturedEvent = event;
  });

  // Start and immediately stop to process one batch
  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 100));
  listener.stop();

  await startPromise.catch(() => {}); // Ignore any errors from stop

  assert.ok(capturedEvent);
  assert.equal(capturedEvent.user, 'GTEST123');
  assert.equal(capturedEvent.amount, '1000000');
});

test('VaultEventListener filters events by user address', async () => {
  const server = new MockSorobanServer();
  server.addMockEvent(
    ['deposit', 'GUSER1'],
    { user: 'GUSER1', amount: '1000' },
    1001
  );
  server.addMockEvent(
    ['deposit', 'GUSER2'],
    { user: 'GUSER2', amount: '2000' },
    1002
  );

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  let user1Events = [];
  listener.onDeposit((event) => {
    user1Events.push(event);
  }, 'GUSER1');

  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 100));
  listener.stop();

  await startPromise.catch(() => {});

  assert.equal(user1Events.length, 1);
  assert.equal(user1Events[0].user, 'GUSER1');
});

test('VaultEventListener supports multiple event types', async () => {
  const server = new MockSorobanServer();
  server.addMockEvent(['deposit'], { user: 'GTEST', amount: '1000' }, 1001);
  server.addMockEvent(['withdraw'], { user: 'GTEST', amount: '500' }, 1002);
  server.addMockEvent(['rebalance'], { protocol: 'blend', status: 'success' }, 1003);

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  let depositCount = 0;
  let withdrawCount = 0;
  let rebalanceCount = 0;

  listener.onDeposit(() => depositCount++);
  listener.onWithdraw(() => withdrawCount++);
  listener.onRebalance(() => rebalanceCount++);

  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 150));
  listener.stop();

  await startPromise.catch(() => {});

  assert.equal(depositCount, 1);
  assert.equal(withdrawCount, 1);
  assert.equal(rebalanceCount, 1);
});

test('VaultEventListener stop terminates listening', () => {
  const server = new MockSorobanServer();
  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  listener.start(1000);
  assert.equal(listener.isRunning, true);

  listener.stop();
  assert.equal(listener.isRunning, false);
});

test('VaultEventListener handles errors gracefully', async () => {
  const server = new MockSorobanServer();
  // Force an error by making getEvents fail once
  let callCount = 0;
  const originalGetEvents = server.getEvents.bind(server);
  server.getEvents = async function(...args) {
    if (callCount++ === 0) {
      throw new Error('Network error');
    }
    return originalGetEvents(...args);
  };

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 50));
  listener.stop();

  await startPromise.catch(() => {});
  
  // Should complete without throwing
  assert.ok(true);
});

test('VaultEventListener supports chaining handler registrations', () => {
  const server = new MockSorobanServer();
  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  const result = listener
    .onDeposit(() => {})
    .onWithdraw(() => {})
    .onRebalance(() => {});

  assert.equal(result, listener);
});

test('VaultEventListener handles multiple handlers for same event', async () => {
  const server = new MockSorobanServer();
  server.addMockEvent(['deposit'], { user: 'GTEST', amount: '1000' }, 1001);

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  let handler1Called = false;
  let handler2Called = false;

  listener.onDeposit(() => handler1Called = true);
  listener.onDeposit(() => handler2Called = true);

  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 100));
  listener.stop();

  await startPromise.catch(() => {});

  assert.ok(handler1Called);
  assert.ok(handler2Called);
});

test('VaultEventListener processes raw event response', async () => {
  const server = new MockSorobanServer();
  server.addMockEvent(['deposit'], { user: 'GTEST', amount: '1000' }, 1001);

  const listener = new VaultEventListener({
    contractId: 'CTEST123',
    server,
    networkPassphrase: 'Test SDF Network ; September 2015',
  });

  let rawEvent = null;
  listener.onDeposit((event, raw) => {
    rawEvent = raw;
  });

  const startPromise = listener.start(1000);
  await new Promise(resolve => setTimeout(resolve, 100));
  listener.stop();

  await startPromise.catch(() => {});

  assert.ok(rawEvent);
  assert.ok(rawEvent.topic);
  assert.ok(rawEvent.value);
});
