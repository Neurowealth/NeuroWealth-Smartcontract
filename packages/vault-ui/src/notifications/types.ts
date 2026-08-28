export type NotificationEventType =
  | 'deposit_confirmed'
  | 'withdrawal_complete'
  | 'rebalance_executed'
  | 'yield_milestone'
  | 'security_alert';

export const NOTIFICATION_EVENT_TYPES: NotificationEventType[] = [
  'deposit_confirmed',
  'withdrawal_complete',
  'rebalance_executed',
  'yield_milestone',
  'security_alert',
];

export interface VaultNotification {
  type: NotificationEventType;
  title: string;
  body: string;
  url?: string;
  createdAt: number;
}

export interface NotificationPreferences {
  enabled: boolean;
  emailFallback: boolean;
  email: string;
  batchWindowMs: number;
  types: Record<NotificationEventType, boolean>;
}

export const DEFAULT_PREFERENCES: NotificationPreferences = {
  enabled: true,
  emailFallback: false,
  email: '',
  batchWindowMs: 60_000,
  types: {
    deposit_confirmed: true,
    withdrawal_complete: true,
    rebalance_executed: true,
    yield_milestone: true,
    security_alert: true,
  },
};

export const PREFERENCES_STORAGE_KEY = 'neurowealth.notification.preferences';

export interface PushDelivery {
  title: string;
  body: string;
  type: NotificationEventType;
  url: string;
  actions: Array<{ action: string; title: string }>;
}

export interface EmailPayload {
  to: string;
  subject: string;
  text: string;
}

export interface PushClient {
  permission: () => NotificationPermission | 'unsupported';
  requestPermission: () => Promise<NotificationPermission | 'unsupported'>;
  show: (delivery: PushDelivery) => Promise<boolean>;
}

export interface EmailAdapter {
  send: (payload: EmailPayload) => Promise<boolean>;
}
