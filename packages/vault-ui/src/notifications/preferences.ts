import {
  DEFAULT_PREFERENCES,
  NOTIFICATION_EVENT_TYPES,
  PREFERENCES_STORAGE_KEY,
  type NotificationEventType,
  type NotificationPreferences,
} from './types';

function isEventType(value: string): value is NotificationEventType {
  return (NOTIFICATION_EVENT_TYPES as string[]).includes(value);
}

export function loadPreferences(storage: Storage | null = defaultStorage()): NotificationPreferences {
  if (!storage) return { ...DEFAULT_PREFERENCES, types: { ...DEFAULT_PREFERENCES.types } };
  try {
    const raw = storage.getItem(PREFERENCES_STORAGE_KEY);
    if (!raw) return { ...DEFAULT_PREFERENCES, types: { ...DEFAULT_PREFERENCES.types } };
    const parsed = JSON.parse(raw) as Partial<NotificationPreferences>;
    const types = { ...DEFAULT_PREFERENCES.types };
    if (parsed.types) {
      for (const key of NOTIFICATION_EVENT_TYPES) {
        if (typeof parsed.types[key] === 'boolean') types[key] = parsed.types[key];
      }
    }
    return {
      enabled: parsed.enabled ?? DEFAULT_PREFERENCES.enabled,
      emailFallback: parsed.emailFallback ?? DEFAULT_PREFERENCES.emailFallback,
      email: typeof parsed.email === 'string' ? parsed.email : '',
      batchWindowMs:
        typeof parsed.batchWindowMs === 'number' && parsed.batchWindowMs >= 0
          ? parsed.batchWindowMs
          : DEFAULT_PREFERENCES.batchWindowMs,
      types,
    };
  } catch {
    return { ...DEFAULT_PREFERENCES, types: { ...DEFAULT_PREFERENCES.types } };
  }
}

export function savePreferences(
  prefs: NotificationPreferences,
  storage: Storage | null = defaultStorage(),
): void {
  storage?.setItem(PREFERENCES_STORAGE_KEY, JSON.stringify(prefs));
}

export function isTypeEnabled(prefs: NotificationPreferences, type: NotificationEventType): boolean {
  if (!prefs.enabled) return false;
  if (!isEventType(type)) return false;
  return prefs.types[type];
}

function defaultStorage(): Storage | null {
  try {
    return typeof localStorage === 'undefined' ? null : localStorage;
  } catch {
    return null;
  }
}
