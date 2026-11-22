import { useEffect, useState } from 'react';
import {
  describeEvent,
  loadPreferences,
  NOTIFICATION_EVENT_TYPES,
  savePreferences,
  type NotificationPreferences,
} from '../notifications';
import { createPushClient } from '../notifications/push-client';

interface NotificationSettingsProps {
  push?: ReturnType<typeof createPushClient>;
}

export default function NotificationSettings({ push = createPushClient() }: NotificationSettingsProps) {
  const [prefs, setPrefs] = useState<NotificationPreferences>(() => loadPreferences());
  const [permission, setPermission] = useState(() => push.permission());
  const [status, setStatus] = useState<string | null>(null);

  useEffect(() => {
    savePreferences(prefs);
  }, [prefs]);

  const enablePush = async () => {
    const result = await push.requestPermission();
    setPermission(result);
    if (result === 'granted') {
      setPrefs(current => ({ ...current, enabled: true }));
      setStatus('Browser notifications are on.');
    } else {
      setStatus('Notifications were blocked. You can still enable email fallback.');
    }
  };

  return (
    <section className="max-w-xl mx-auto bg-white rounded-xl shadow-sm border border-gray-200 p-6" aria-labelledby="notification-settings-heading">
      <h2 id="notification-settings-heading" className="text-2xl font-semibold text-gray-900 mb-2">
        Notifications
      </h2>
      <p className="text-base text-gray-700 mb-6">
        Choose how NeuroWealth tells you about deposits, withdrawals, rebalances, yield, and security events.
      </p>

      <div className="flex flex-col gap-3 mb-6">
        <button
          type="button"
          onClick={enablePush}
          className="w-full py-3 rounded-lg bg-primary-700 text-white font-semibold hover:bg-primary-700 focus-visible:outline focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-primary-700"
        >
          {permission === 'granted' ? 'Browser notifications enabled' : 'Enable browser notifications'}
        </button>
        <p className="text-sm text-gray-700" aria-live="polite">
          Permission: {permission}
        </p>
      </div>

      <fieldset className="mb-6">
        <legend className="text-sm font-medium text-gray-900 mb-2">Event types</legend>
        <ul className="space-y-3">
          {NOTIFICATION_EVENT_TYPES.map(type => {
            const copy = describeEvent(type);
            const id = `notify-${type}`;
            return (
              <li key={type}>
                <label htmlFor={id} className="flex items-start gap-3 text-gray-900">
                  <input
                    id={id}
                    type="checkbox"
                    className="mt-1 h-4 w-4"
                    checked={prefs.types[type]}
                    onChange={e =>
                      setPrefs(current => ({
                        ...current,
                        types: { ...current.types, [type]: e.target.checked },
                      }))
                    }
                  />
                  <span>
                    <span className="block font-medium">{copy.label}</span>
                    <span className="block text-sm text-gray-700">{copy.description}</span>
                  </span>
                </label>
              </li>
            );
          })}
        </ul>
      </fieldset>

      <fieldset className="mb-6">
        <legend className="text-sm font-medium text-gray-900 mb-2">Email fallback</legend>
        <label htmlFor="email-fallback" className="flex items-center gap-3 text-gray-900 mb-3">
          <input
            id="email-fallback"
            type="checkbox"
            className="h-4 w-4"
            checked={prefs.emailFallback}
            onChange={e => setPrefs(current => ({ ...current, emailFallback: e.target.checked }))}
          />
          Send email when push is unavailable
        </label>
        <label htmlFor="notify-email" className="block text-sm font-medium text-gray-900 mb-1">
          Email address
        </label>
        <input
          id="notify-email"
          type="email"
          autoComplete="email"
          value={prefs.email}
          onChange={e => setPrefs(current => ({ ...current, email: e.target.value }))}
          className="w-full rounded-lg border border-gray-300 px-3 py-2 text-gray-900"
          placeholder="you@example.com"
        />
      </fieldset>

      <label htmlFor="batch-window" className="block text-sm font-medium text-gray-900 mb-1">
        Batch window (seconds)
      </label>
      <input
        id="batch-window"
        type="number"
        min={0}
        step={15}
        value={Math.round(prefs.batchWindowMs / 1000)}
        onChange={e =>
          setPrefs(current => ({
            ...current,
            batchWindowMs: Math.max(0, Number(e.target.value) || 0) * 1000,
          }))
        }
        className="w-full rounded-lg border border-gray-300 px-3 py-2 text-gray-900 mb-4"
      />
      <p className="text-sm text-gray-700 mb-4">
        Events of the same type that arrive inside this window are grouped into one notification. Security alerts are never delayed.
      </p>

      {status && (
        <p className="text-sm text-gray-900" role="status" aria-live="polite">
          {status}
        </p>
      )}
    </section>
  );
}
