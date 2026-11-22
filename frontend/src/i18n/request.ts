import { getRequestConfig } from 'next-intl/server';
import { notFound } from 'next/navigation';

export const locales = ['en', 'es', 'fr', 'pt', 'zh', 'ja'];

export default getRequestConfig(async ({ locale }) => {
  const requestedLocale = locale ?? 'en';
  if (!locales.includes(requestedLocale)) notFound();

  return {
    locale: requestedLocale,
    messages: (await import(`../../messages/${requestedLocale}.json`)).default
  };
});
