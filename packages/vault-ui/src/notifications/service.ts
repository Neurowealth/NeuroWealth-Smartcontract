import { formatBatch, NotificationBatcher, type BatchFlush } from './batcher';
import { isTypeEnabled, loadPreferences } from './preferences';
import { deliveryFrom } from './push-client';
import type {
  EmailAdapter,
  NotificationEventType,
  NotificationPreferences,
  PushClient,
  VaultNotification,
} from './types';

export interface NotificationServiceOptions {
  push: PushClient;
  email: EmailAdapter;
  loadPrefs?: () => NotificationPreferences;
  now?: () => number;
}

/**
 * Fan-out helper: preference check → batch → push, with email fallback.
 */
export class NotificationService {
  private readonly push: PushClient;
  private readonly email: EmailAdapter;
  private readonly loadPrefs: () => NotificationPreferences;
  private batcher: NotificationBatcher;
  private windowMs: number;

  constructor(options: NotificationServiceOptions) {
    this.push = options.push;
    this.email = options.email;
    this.loadPrefs = options.loadPrefs ?? loadPreferences;
    this.windowMs = this.loadPrefs().batchWindowMs;
    this.batcher = new NotificationBatcher(this.windowMs, flush => {
      void this.deliver(flush);
    });
  }

  notify(event: Omit<VaultNotification, 'createdAt'> & { createdAt?: number }): void {
    const prefs = this.loadPrefs();
    if (!isTypeEnabled(prefs, event.type)) return;

    if (prefs.batchWindowMs !== this.windowMs) {
      this.batcher.flush();
      this.windowMs = prefs.batchWindowMs;
      this.batcher = new NotificationBatcher(this.windowMs, flush => {
        void this.deliver(flush);
      });
    }

    this.batcher.enqueue({
      ...event,
      createdAt: event.createdAt ?? Date.now(),
    });
  }

  flush(): void {
    this.batcher.flush();
  }

  private async deliver(flush: BatchFlush): Promise<void> {
    const prefs = this.loadPrefs();
    const { title, body } = formatBatch(flush);
    const shown = await this.push.show(
      deliveryFrom(title, body, flush.type, flush.events[0]?.url ?? '/'),
    );
    if (!shown && prefs.emailFallback && prefs.email) {
      await this.email.send({
        to: prefs.email,
        subject: title,
        text: body,
      });
    }
  }
}

export function describeEvent(type: NotificationEventType): { label: string; description: string } {
  switch (type) {
    case 'deposit_confirmed':
      return { label: 'Deposit confirmed', description: 'When your USDC deposit is recorded on-chain.' };
    case 'withdrawal_complete':
      return { label: 'Withdrawal complete', description: 'When a withdrawal finishes and USDC is back in your wallet.' };
    case 'rebalance_executed':
      return { label: 'Rebalance executed', description: 'When the agent moves funds between strategies.' };
    case 'yield_milestone':
      return { label: 'Yield milestone', description: 'When your share value crosses a yield milestone.' };
    case 'security_alert':
      return { label: 'Security alert', description: 'Pause, agent rotation, or other high-severity events.' };
  }
}
