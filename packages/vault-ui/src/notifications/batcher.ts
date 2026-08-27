import type { NotificationEventType, VaultNotification } from './types';

export interface BatchFlush {
  type: NotificationEventType;
  events: VaultNotification[];
}

/**
 * Groups notifications of the same type that arrive inside `windowMs`.
 * Security alerts ignore the window and flush immediately.
 */
export class NotificationBatcher {
  private readonly buckets = new Map<NotificationEventType, VaultNotification[]>();
  private readonly timers = new Map<NotificationEventType, ReturnType<typeof setTimeout>>();
  private readonly windowMs: number;
  private readonly onFlush: (flush: BatchFlush) => void;

  constructor(windowMs: number, onFlush: (flush: BatchFlush) => void) {
    this.windowMs = windowMs;
    this.onFlush = onFlush;
  }

  enqueue(event: VaultNotification): void {
    const delay = event.type === 'security_alert' ? 0 : this.windowMs;
    const existing = this.buckets.get(event.type) ?? [];
    existing.push(event);
    this.buckets.set(event.type, existing);

    const pending = this.timers.get(event.type);
    if (pending) return;

    const timer = setTimeout(() => this.flush(event.type), delay);
    this.timers.set(event.type, timer);
  }

  flush(type?: NotificationEventType): void {
    const types = type ? [type] : [...this.buckets.keys()];
    for (const t of types) {
      const timer = this.timers.get(t);
      if (timer) clearTimeout(timer);
      this.timers.delete(t);
      const events = this.buckets.get(t);
      this.buckets.delete(t);
      if (events && events.length > 0) {
        this.onFlush({ type: t, events });
      }
    }
  }

  clear(): void {
    for (const timer of this.timers.values()) clearTimeout(timer);
    this.timers.clear();
    this.buckets.clear();
  }
}

export function formatBatch(flush: BatchFlush): { title: string; body: string } {
  const first = flush.events[0];
  if (flush.events.length === 1) {
    return { title: first.title, body: first.body };
  }
  return {
    title: first.title,
    body: `${flush.events.length} ${flush.type.replace(/_/g, ' ')} updates. Latest: ${first.body}`,
  };
}
