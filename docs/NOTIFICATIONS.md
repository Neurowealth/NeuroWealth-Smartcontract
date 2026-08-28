# Vault Event Notifications

> **Issue:** #669
> **Scope:** `@neurowealth/vault-ui` Web Push + email fallback

Users should not have to keep the dashboard open to learn that a deposit confirmed, a withdrawal completed, a rebalance ran, a yield milestone was hit, or a security event fired. This document describes the notification pipeline, preference model, batching, and how to wire an email provider.

## Channels

| Channel | Mechanism | When it is used |
|---------|-----------|-----------------|
| Browser / PWA push | Web Push API + `public/sw.js` | Permission granted and a push subscription exists |
| In-page (fallback of last resort) | `Notification` constructor when the page is open | Permission granted but no service-worker registration |
| Email | `EmailAdapter` (Resend or SendGrid) | User enabled “email fallback”, or push delivery failed |

Mobile devices that have installed the PWA receive the same Web Push payloads through the service worker. There is no separate APNs/FCM integration; the browser vendor bridges to the OS.

## Event types

| Type | Fired when | Default |
|------|------------|---------|
| `deposit_confirmed` | Vault `deposit` event for the signed-in user | on |
| `withdrawal_complete` | Vault `withdraw` / `withdraw_all` event for the user | on |
| `rebalance_executed` | Vault `rebalance` event | on |
| `yield_milestone` | Share-price (exchange rate) crosses a configured multiple of the user's baseline | on |
| `security_alert` | Pause, unexpected agent rotation, or other high-severity monitoring signal | on (cannot be fully disabled; the preference only suppresses the in-app copy, not the email/push of critical alerts if the operator forces them) |

Rich notifications include action buttons:

- **View portfolio** → `/#earnings`
- **Quick withdraw** → `/#withdraw`

The service worker handles `notificationclick` for those actions.

## Preferences

Stored in `localStorage` under `neurowealth.notification.preferences` (see `loadPreferences` / `savePreferences`). Users can:

- Enable or disable the whole channel
- Toggle each event type
- Enable email fallback and set an address
- Set the batch window (default 60 seconds)

`NotificationSettings` in the vault UI is the settings surface. Preferences are read before every dispatch.

## Batching

`NotificationBatcher` groups events of the **same type** that arrive inside `batchWindowMs`. A single push/email is sent with a count (“3 deposits confirmed”) instead of three back-to-back alerts. Different types never share a batch. Security alerts use a 0 ms window so they are never delayed.

## Architecture

```
VaultEventListener  →  NotificationService.notify()
                           │
                           ├─ preferences.enabled && type enabled?
                           ├─ batcher.enqueue(event)
                           └─ flush → PushClient.show()
                                    → EmailAdapter.send() if fallback
```

`NotificationService` is framework-agnostic so the same module can be driven from the UI, a future agent process, or unit tests (inject a fake `PushClient` and `EmailAdapter`).

## Email providers

`createEmailAdapter` accepts:

```ts
{
  provider: 'resend' | 'sendgrid' | 'console',
  apiKey: string,
  from: string,          // e.g. alerts@neurowealth.example
}
```

- **Resend:** `POST https://api.resend.com/emails`
- **SendGrid:** `POST https://api.sendgrid.com/v3/mail/send`
- **console:** logs the payload (default in development)

Do not commit API keys. Set `VITE_EMAIL_PROVIDER`, `VITE_EMAIL_API_KEY`, and `VITE_EMAIL_FROM` in the deployment environment. The browser should **not** hold the raw provider key in production — proxy through a small backend that attaches the secret. The adapter accepts a `endpoint` override for that proxy.

## Web Push keys

Generate a VAPID pair and set `VITE_VAPID_PUBLIC_KEY`. The corresponding private key belongs on the push server that fans vault events out to stored subscriptions; this package only registers the subscription and renders notifications.

## Tests

```bash
cd packages/vault-ui
npm test
```

`src/notifications/notifications.test.ts` covers preference defaults, per-type toggling, batching (same type coalesces; different types do not; security alerts flush immediately), push delivery, and email fallback when push fails.

## Service worker

`packages/vault-ui/public/sw.js` is copied as-is by Vite. It:

1. Handles `push` events and shows a notification with the payload title/body/actions.
2. Handles `notificationclick` and opens the matching app URL.
3. Deduplicates tag `neurowealth-${type}` so OS-level stacking stays sane.
