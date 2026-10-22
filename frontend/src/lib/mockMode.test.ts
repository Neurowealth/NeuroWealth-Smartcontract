import { afterEach, describe, expect, it, vi } from 'vitest';
import { isMockModeEnabled } from './mockMode';
import { getBrowserTestHarness } from './browserTestHarness';

describe('mock mode (#930)', () => {
  afterEach(() => {
    vi.unstubAllEnvs();
    delete window.__NEUROWEALTH_E2E__;
  });

  it('is off by default', () => {
    vi.stubEnv('NEXT_PUBLIC_USE_MOCKS', '');
    vi.stubEnv('NEXT_PUBLIC_E2E_TESTS', '');
    expect(isMockModeEnabled()).toBe(false);
  });

  it('is enabled explicitly by NEXT_PUBLIC_USE_MOCKS or the E2E alias', () => {
    vi.stubEnv('NEXT_PUBLIC_USE_MOCKS', 'true');
    expect(isMockModeEnabled()).toBe(true);
    vi.stubEnv('NEXT_PUBLIC_USE_MOCKS', '');
    vi.stubEnv('NEXT_PUBLIC_E2E_TESTS', 'true');
    expect(isMockModeEnabled()).toBe(true);
  });

  it('ignores an injected harness unless mock mode is on', () => {
    vi.stubEnv('NEXT_PUBLIC_USE_MOCKS', '');
    vi.stubEnv('NEXT_PUBLIC_E2E_TESTS', '');
    window.__NEUROWEALTH_E2E__ = {} as NonNullable<Window['__NEUROWEALTH_E2E__']>;
    expect(getBrowserTestHarness()).toBeNull();

    vi.stubEnv('NEXT_PUBLIC_USE_MOCKS', 'true');
    expect(getBrowserTestHarness()).toBe(window.__NEUROWEALTH_E2E__);
  });
});
