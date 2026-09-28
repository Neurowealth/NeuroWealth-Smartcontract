import { act, renderHook } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { useRealtimePortfolio } from './useRealtimePortfolio';

class MockWebSocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static instances: MockWebSocket[] = [];

  readyState = MockWebSocket.CONNECTING;
  onopen: (() => void) | null = null;
  onclose: (() => void) | null = null;
  onmessage: ((event: { data: string }) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  sent: string[] = [];

  constructor(public url: string) {
    MockWebSocket.instances.push(this);
  }

  send(data: string) {
    this.sent.push(data);
  }

  close() {
    if (this.readyState === MockWebSocket.CLOSED) return;
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.();
  }

  triggerOpen() {
    this.readyState = MockWebSocket.OPEN;
    this.onopen?.();
  }

  triggerMessage(data: unknown) {
    this.onmessage?.({ data: JSON.stringify(data) });
  }

  triggerClose() {
    this.readyState = MockWebSocket.CLOSED;
    this.onclose?.();
  }

  sentActions() {
    return this.sent.map(s => JSON.parse(s));
  }
}

function mockFetchOk() {
  return vi.fn().mockResolvedValue({
    ok: true,
    json: async () => ({
      balance: { idle: 1, deployed: 2, total: 3 },
      yield: { rate: 0.1, earned24h: 0.01, earnedTotal: 0.5 },
    }),
  });
}

describe('useRealtimePortfolio', () => {
  beforeEach(() => {
    vi.useFakeTimers();
    MockWebSocket.instances = [];
    // @ts-expect-error test override of the global WebSocket implementation
    global.WebSocket = MockWebSocket;
    global.fetch = mockFetchOk();
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('reconnects with bounded exponential backoff, resubscribes, and resyncs after reconnect', async () => {
    const { result } = renderHook(() =>
      useRealtimePortfolio({ contractId: 'C123', accountAddress: 'GACCOUNT' })
    );

    expect(MockWebSocket.instances).toHaveLength(1);
    const first = MockWebSocket.instances[0];

    act(() => {
      first.triggerOpen();
    });

    expect(result.current.isConnected).toBe(true);
    expect(result.current.connectionStatus).toBe('connected');
    expect(first.sentActions()).toEqual(
      expect.arrayContaining([
        { action: 'subscribe', contractId: 'C123' },
        { action: 'subscribe_account', address: 'GACCOUNT' },
      ])
    );

    // Connection drops.
    act(() => {
      first.triggerClose();
    });

    expect(result.current.isConnected).toBe(false);
    expect(result.current.connectionStatus).toBe('reconnecting');
    expect(result.current.isStale).toBe(true);
    // No second socket yet - backoff hasn't elapsed.
    expect(MockWebSocket.instances).toHaveLength(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    expect(MockWebSocket.instances).toHaveLength(2);
    const second = MockWebSocket.instances[1];

    const fetchCallsBeforeReconnect = (global.fetch as ReturnType<typeof vi.fn>).mock.calls.length;

    act(() => {
      second.triggerOpen();
    });

    expect(second.sentActions()).toEqual(
      expect.arrayContaining([
        { action: 'subscribe', contractId: 'C123' },
        { action: 'subscribe_account', address: 'GACCOUNT' },
      ])
    );

    // A reconnect must resync from the authoritative source, not just wait for the next push.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect((global.fetch as ReturnType<typeof vi.fn>).mock.calls.length).toBeGreaterThan(
      fetchCallsBeforeReconnect
    );
    expect(result.current.connectionStatus).toBe('connected');
  });

  it('gives up after the bounded number of attempts and falls back to a polling read', async () => {
    const { result } = renderHook(() =>
      useRealtimePortfolio({
        contractId: 'C123',
        accountAddress: 'GACCOUNT',
        maxReconnectAttempts: 2,
        pollIntervalMs: 5000,
      })
    );

    // Fail three times in a row without ever completing a handshake.
    act(() => {
      MockWebSocket.instances[0].triggerClose();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });
    expect(MockWebSocket.instances).toHaveLength(2);

    act(() => {
      MockWebSocket.instances[1].triggerClose();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(MockWebSocket.instances).toHaveLength(3);

    act(() => {
      MockWebSocket.instances[2].triggerClose();
    });

    // Terminal failure: no further socket is created, and the hook falls back to polling
    // after an immediate authoritative refresh.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(0);
    });
    expect(result.current.connectionType).toBe('polling');
    expect(result.current.isConnected).toBe(true);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(10);
    });
    expect(MockWebSocket.instances).toHaveLength(3);
  });

  it('cancels the previous subscription before applying data for a switched account', async () => {
    const { result, rerender } = renderHook(
      ({ accountAddress }: { accountAddress: string }) =>
        useRealtimePortfolio({ contractId: 'C123', accountAddress }),
      { initialProps: { accountAddress: 'GACCOUNT_A' } }
    );

    const first = MockWebSocket.instances[0];
    act(() => {
      first.triggerOpen();
    });

    act(() => {
      first.triggerMessage({
        type: 'vault_event',
        eventType: 'deposit',
        amount: '10',
        asset: 'USDC',
        ledger: 1,
        txHash: 'tx-a',
      });
    });
    expect(result.current.events).toHaveLength(1);

    rerender({ accountAddress: 'GACCOUNT_B' });

    // The old socket must be closed as part of the switch...
    expect(first.readyState).toBe(MockWebSocket.CLOSED);
    // ...and a single fresh socket opened for the new account.
    expect(MockWebSocket.instances).toHaveLength(2);
    const second = MockWebSocket.instances[1];

    // Events from the previous subscription don't leak into the new one.
    expect(result.current.events).toHaveLength(0);

    act(() => {
      second.triggerOpen();
    });
    expect(second.sentActions()).toEqual(
      expect.arrayContaining([{ action: 'subscribe_account', address: 'GACCOUNT_B' }])
    );

    // Closing the old socket must not have scheduled a reconnect for the old account.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(30000);
    });
    expect(MockWebSocket.instances).toHaveLength(2);
  });

  it('drops duplicate vault events replayed after a resubscribe', async () => {
    const { result } = renderHook(() =>
      useRealtimePortfolio({ contractId: 'C123', accountAddress: 'GACCOUNT' })
    );

    const first = MockWebSocket.instances[0];
    act(() => {
      first.triggerOpen();
      first.triggerMessage({
        type: 'vault_event',
        eventType: 'yield',
        amount: '1',
        asset: 'USDC',
        ledger: 5,
        txHash: 'tx-dup',
      });
    });
    expect(result.current.events).toHaveLength(1);

    act(() => {
      first.triggerClose();
    });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(1000);
    });

    const second = MockWebSocket.instances[1];
    await act(async () => {
      second.triggerOpen();
      // Server replays the same event on resubscribe.
      second.triggerMessage({
        type: 'vault_event',
        eventType: 'yield',
        amount: '1',
        asset: 'USDC',
        ledger: 5,
        txHash: 'tx-dup',
      });
      await vi.advanceTimersByTimeAsync(0);
    });

    expect(result.current.events).toHaveLength(1);
  });
});
