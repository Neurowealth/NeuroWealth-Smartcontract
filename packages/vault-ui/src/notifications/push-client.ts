import type { PushClient, PushDelivery } from './types';

const ACTIONS: PushDelivery['actions'] = [
  { action: 'view-portfolio', title: 'View portfolio' },
  { action: 'quick-withdraw', title: 'Quick withdraw' },
];

export function createPushClient(): PushClient {
  return {
    permission() {
      if (typeof Notification === 'undefined') return 'unsupported';
      return Notification.permission;
    },
    async requestPermission() {
      if (typeof Notification === 'undefined') return 'unsupported';
      return Notification.requestPermission();
    },
    async show(delivery: PushDelivery) {
      if (typeof Notification === 'undefined') return false;
      if (Notification.permission !== 'granted') return false;

      if (typeof navigator !== 'undefined' && 'serviceWorker' in navigator) {
        const registration = await navigator.serviceWorker.getRegistration();
        if (registration) {
          await registration.showNotification(delivery.title, {
            body: delivery.body,
            tag: `neurowealth-${delivery.type}`,
            data: { url: delivery.url, type: delivery.type },
            actions: delivery.actions,
          } as NotificationOptions);
          return true;
        }
      }

      new Notification(delivery.title, { body: delivery.body });
      return true;
    },
  };
}

export function deliveryFrom(
  title: string,
  body: string,
  type: PushDelivery['type'],
  url = '/',
): PushDelivery {
  return { title, body, type, url, actions: ACTIONS };
}
