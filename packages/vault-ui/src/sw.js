import { precacheAndRoute } from 'workbox-precaching';

// Workbox injects the precache manifest here at build time (injectManifest
// strategy). This keeps the original generateSW precaching behavior while
// allowing the custom push/notification handlers below to live in the same
// service worker, avoiding the sw.js path collision (#1183).
precacheAndRoute(self.__WB_MANIFEST);

/* NeuroWealth vault notification service worker (Issue #669). */

self.addEventListener('push', event => {
  let payload = { title: 'NeuroWealth', body: 'Vault update', url: '/', type: 'deposit_confirmed' };
  try {
    if (event.data) payload = { ...payload, ...event.data.json() };
  } catch {
    if (event.data) payload.body = event.data.text();
  }

  event.waitUntil(
    self.registration.showNotification(payload.title, {
      body: payload.body,
      tag: `neurowealth-${payload.type}`,
      data: { url: payload.url, type: payload.type },
      actions: [
        { action: 'view-portfolio', title: 'View portfolio' },
        { action: 'quick-withdraw', title: 'Quick withdraw' },
      ],
    }),
  );
});

self.addEventListener('notificationclick', event => {
  event.notification.close();
  let url = '/';
  if (event.action === 'view-portfolio') url = '/#earnings';
  else if (event.action === 'quick-withdraw') url = '/#withdraw';
  else if (event.notification.data && event.notification.data.url) url = event.notification.data.url;

  event.waitUntil(
    self.clients.matchAll({ type: 'window', includeUncontrolled: true }).then(clients => {
      for (const client of clients) {
        if ('focus' in client) {
          client.navigate?.(url);
          return client.focus();
        }
      }
      if (self.clients.openWindow) return self.clients.openWindow(url);
      return undefined;
    }),
  );
});
