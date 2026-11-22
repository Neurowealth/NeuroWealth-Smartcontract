import { describe, expect, it, vi } from 'vitest';
import { NotificationBatcher, formatBatch } from './batcher';
import { createEmailAdapter } from './email-fallback';
import { isTypeEnabled, loadPreferences, savePreferences } from './preferences';
import { NotificationService } from './service';
import {
  DEFAULT_PREFERENCES,
  PREFERENCES_STORAGE_KEY,
  type PushDelivery,
  type VaultNotification,
} from './types';

function memoryStorage(initial: Record<string, string> = {}): Storage {
  const map = new Map(Object.entries(initial));
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => {
      map.delete(key);
    },
    setItem: (key: string, value: string) => {
      map.set(key, value);
    },
  };
}

describe('notification preferences', () => {
  it('loads defaults when storage is empty', () => {
    const prefs = loadPreferences(memoryStorage());
    expect(prefs.enabled).toBe(true);
    expect(prefs.types.deposit_confirmed).toBe(true);
    expect(prefs.batchWindowMs).toBe(60_000);
  });

  it('round-trips saved preferences', () => {
    const storage = memoryStorage();
    const next = {
      ...DEFAULT_PREFERENCES,
      emailFallback: true,
      email: 'user@example.com',
      types: { ...DEFAULT_PREFERENCES.types, rebalance_executed: false },
    };
    savePreferences(next, storage);
    expect(storage.getItem(PREFERENCES_STORAGE_KEY)).toContain('user@example.com');
    const loaded = loadPreferences(storage);
    expect(loaded.emailFallback).toBe(true);
    expect(isTypeEnabled(loaded, 'rebalance_executed')).toBe(false);
    expect(isTypeEnabled(loaded, 'deposit_confirmed')).toBe(true);
  });

  it('disables every type when the master switch is off', () => {
    const prefs = { ...DEFAULT_PREFERENCES, enabled: false, types: { ...DEFAULT_PREFERENCES.types } };
    expect(isTypeEnabled(prefs, 'security_alert')).toBe(false);
  });
});

describe('notification batching', () => {
  it('groups same-type events inside the window', () => {
    vi.useFakeTimers();
    const flushed: ReturnType<typeof formatBatch>[] = [];
    const batcher = new NotificationBatcher(1_000, flush => {
      flushed.push(formatBatch(flush));
    });
    const event = (body: string): VaultNotification => ({
      type: 'deposit_confirmed',
      title: 'Deposit confirmed',
      body,
      createdAt: 1,
    });
    batcher.enqueue(event('one'));
    batcher.enqueue(event('two'));
    expect(flushed).toHaveLength(0);
    vi.advanceTimersByTime(1_000);
    expect(flushed).toHaveLength(1);
    expect(flushed[0].body).toContain('2 deposit confirmed');
    vi.useRealTimers();
  });

  it('does not mix different event types in one batch', () => {
    const flushes: string[] = [];
    const batcher = new NotificationBatcher(60_000, flush => {
      flushes.push(flush.type);
    });
    batcher.enqueue({
      type: 'deposit_confirmed',
      title: 'd',
      body: 'd',
      createdAt: 1,
    });
    batcher.enqueue({
      type: 'rebalance_executed',
      title: 'r',
      body: 'r',
      createdAt: 1,
    });
    batcher.flush();
    expect(flushes.sort()).toEqual(['deposit_confirmed', 'rebalance_executed']);
  });

  it('flushes security alerts immediately', () => {
    vi.useFakeTimers();
    const flushes: string[] = [];
    const batcher = new NotificationBatcher(60_000, flush => {
      flushes.push(flush.type);
    });
    batcher.enqueue({
      type: 'security_alert',
      title: 'Paused',
      body: 'Vault paused',
      createdAt: 1,
    });
    vi.advanceTimersByTime(0);
    expect(flushes).toEqual(['security_alert']);
    vi.useRealTimers();
  });
});

describe('notification delivery', () => {
  it('shows a push notification when permission is granted', async () => {
    const shown: PushDelivery[] = [];
    const service = new NotificationService({
      push: {
        permission: () => 'granted',
        requestPermission: async () => 'granted',
        show: async delivery => {
          shown.push(delivery);
          return true;
        },
      },
      email: { send: async () => false },
      loadPrefs: () => ({ ...DEFAULT_PREFERENCES, types: { ...DEFAULT_PREFERENCES.types }, batchWindowMs: 0 }),
    });
    service.notify({ type: 'withdrawal_complete', title: 'Withdrawal complete', body: '10 USDC' });
    service.flush();
    await vi.waitFor(() => expect(shown).toHaveLength(1));
    expect(shown[0].actions.map(a => a.action)).toEqual(['view-portfolio', 'quick-withdraw']);
  });

  it('falls back to email when push delivery fails', async () => {
    const emails: string[] = [];
    const service = new NotificationService({
      push: {
        permission: () => 'denied',
        requestPermission: async () => 'denied',
        show: async () => false,
      },
      email: {
        send: async payload => {
          emails.push(payload.to);
          return true;
        },
      },
      loadPrefs: () => ({
        ...DEFAULT_PREFERENCES,
        types: { ...DEFAULT_PREFERENCES.types },
        batchWindowMs: 0,
        emailFallback: true,
        email: 'user@example.com',
      }),
    });
    service.notify({ type: 'yield_milestone', title: 'Yield', body: '8%' });
    service.flush();
    await vi.waitFor(() => expect(emails).toEqual(['user@example.com']));
  });

  it('does not deliver disabled event types', async () => {
    const shown: PushDelivery[] = [];
    const service = new NotificationService({
      push: {
        permission: () => 'granted',
        requestPermission: async () => 'granted',
        show: async delivery => {
          shown.push(delivery);
          return true;
        },
      },
      email: { send: async () => true },
      loadPrefs: () => ({
        ...DEFAULT_PREFERENCES,
        types: { ...DEFAULT_PREFERENCES.types, deposit_confirmed: false },
        batchWindowMs: 0,
      }),
    });
    service.notify({ type: 'deposit_confirmed', title: 'Deposit', body: '1' });
    service.flush();
    await Promise.resolve();
    expect(shown).toHaveLength(0);
  });
});

describe('email adapter', () => {
  it('posts to Resend when configured', async () => {
    const calls: string[] = [];
    const adapter = createEmailAdapter({
      provider: 'resend',
      apiKey: 're_test',
      from: 'alerts@example.com',
      fetchImpl: (async (url: RequestInfo | URL) => {
        calls.push(String(url));
        return new Response('{}', { status: 200 });
      }) as typeof fetch,
    });
    const ok = await adapter.send({ to: 'u@example.com', subject: 'Hi', text: 'body' });
    expect(ok).toBe(true);
    expect(calls[0]).toContain('resend.com');
  });
});
