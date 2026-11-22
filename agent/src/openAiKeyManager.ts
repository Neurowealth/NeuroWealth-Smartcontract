import OpenAI from 'openai';
import logger from './logger';

export interface KeyStatus {
  keyIndex: number;
  maskedKey: string;
  isHealthy: boolean;
  failureCount: number;
  lastFailedAt?: number;
  lastError?: string;
}

export class OpenAIKeyManager {
  private keys: string[] = [];
  private keyState: {
    isHealthy: boolean;
    failureCount: number;
    lastFailedAt?: number;
    lastError?: string;
  }[] = [];
  private currentIndex = 0;
  private defaultCooldownMs = 60 * 1000; // 1 minute cooldown

  constructor(customKeys?: string[]) {
    if (customKeys && customKeys.length > 0) {
      this.keys = customKeys;
    } else {
      const envKeys = process.env.OPENAI_API_KEYS || process.env.OPENAI_API_KEY || '';
      this.keys = envKeys
        .split(',')
        .map((k) => k.trim())
        .filter((k) => k.length > 0);
    }

    if (this.keys.length === 0) {
      logger.warn('No OpenAI API keys configured in OPENAI_API_KEYS or OPENAI_API_KEY');
    }

    this.keyState = this.keys.map(() => ({
      isHealthy: true,
      failureCount: 0,
    }));
  }

  public get keyCount(): number {
    return this.keys.length;
  }

  public getHealthStatus(): KeyStatus[] {
    const now = Date.now();
    return this.keys.map((key, i) => {
      const state = this.keyState[i];
      // Check if cooldown has elapsed
      const isCoolingDown =
        !state.isHealthy &&
        state.lastFailedAt &&
        now - state.lastFailedAt < this.defaultCooldownMs;

      return {
        keyIndex: i,
        maskedKey: this.maskKey(key),
        isHealthy: state.isHealthy || !isCoolingDown,
        failureCount: state.failureCount,
        lastFailedAt: state.lastFailedAt,
        lastError: state.lastError,
      };
    });
  }

  private maskKey(key: string): string {
    if (key.length <= 8) return '***';
    return `${key.slice(0, 4)}...${key.slice(-4)}`;
  }

  private getNextAvailableKeyIndex(): number | null {
    if (this.keys.length === 0) return null;

    const now = Date.now();
    const total = this.keys.length;

    for (let attempt = 0; attempt < total; attempt++) {
      const idx = (this.currentIndex + attempt) % total;
      const state = this.keyState[idx];

      // Auto-recover key if cooldown expired
      if (!state.isHealthy && state.lastFailedAt && now - state.lastFailedAt >= this.defaultCooldownMs) {
        state.isHealthy = true;
        logger.info({ keyIndex: idx }, 'OpenAI API key recovered from cooldown');
      }

      if (state.isHealthy) {
        this.currentIndex = (idx + 1) % total;
        return idx;
      }
    }

    // Fallback: if all keys are cooling down, use the key that failed longest ago
    let oldestFailedIdx = 0;
    let oldestTime = Infinity;
    for (let i = 0; i < total; i++) {
      const time = this.keyState[i].lastFailedAt || 0;
      if (time < oldestTime) {
        oldestTime = time;
        oldestFailedIdx = i;
      }
    }

    logger.warn({ fallbackIndex: oldestFailedIdx }, 'All OpenAI API keys unhealthy, attempting fallback key');
    return oldestFailedIdx;
  }

  public async executeWithRotation<T>(
    operation: (client: OpenAI) => Promise<T>
  ): Promise<T> {
    if (this.keys.length === 0) {
      throw new Error('No OpenAI API keys configured');
    }

    const attempts = this.keys.length;
    const errors: string[] = [];

    for (let i = 0; i < attempts; i++) {
      const keyIndex = this.getNextAvailableKeyIndex();
      if (keyIndex === null) break;

      const apiKey = this.keys[keyIndex];
      const client = new OpenAI({ apiKey });

      try {
        const result = await operation(client);
        // Reset failure count on success
        this.keyState[keyIndex].isHealthy = true;
        this.keyState[keyIndex].failureCount = 0;
        return result;
      } catch (error: any) {
        const status = error?.status || error?.response?.status;
        const message = error instanceof Error ? error.message : String(error);

        this.keyState[keyIndex].failureCount += 1;
        this.keyState[keyIndex].lastFailedAt = Date.now();
        this.keyState[keyIndex].lastError = message;

        // Mark key unhealthy on 401, 429, or 5xx errors
        if (status === 401 || status === 429 || (status >= 500 && status < 600) || message.includes('rate limit') || message.includes('quota')) {
          this.keyState[keyIndex].isHealthy = false;
          logger.warn(
            { keyIndex: this.maskKey(apiKey), status, error: message },
            'OpenAI API key failed, rotating to next key'
          );
        }

        errors.push(`Key ${this.maskKey(apiKey)} failed: ${message}`);
      }
    }

    throw new Error(`All OpenAI API keys failed: ${errors.join('; ')}`);
  }
}

export const openAiKeyManager = new OpenAIKeyManager();
