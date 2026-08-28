export type { NotificationEventType, NotificationPreferences, VaultNotification } from './types';
export { DEFAULT_PREFERENCES, NOTIFICATION_EVENT_TYPES } from './types';
export { loadPreferences, savePreferences, isTypeEnabled } from './preferences';
export { NotificationBatcher, formatBatch } from './batcher';
export { createPushClient, deliveryFrom } from './push-client';
export { createEmailAdapter } from './email-fallback';
export { NotificationService, describeEvent } from './service';
