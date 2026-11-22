import type { Metadata } from 'next';
import '../globals.css';
import { ThemeProvider } from '@/components/ThemeProvider';
import { OnboardingTutorial } from '@/components/OnboardingTutorial';
import { NextIntlClientProvider } from 'next-intl';
import { getMessages } from 'next-intl/server';
import { notFound } from 'next/navigation';
import { locales } from '@/i18n/request';

export const metadata: Metadata = {
  title: 'NeuroWealth | AI-Powered DeFi Yield Platform on Stellar',
  description: 'Autonomous AI investment agent managing smart contract yield strategies on the Stellar blockchain with 24/7 rebalancing.',
};

export default async function RootLayout({
  children,
  params
}: {
  children: React.ReactNode;
  params: Promise<{ locale: string }>;
}) {
  const { locale } = await params;
  if (!locales.includes(locale as any)) notFound();
  
  const messages = await getMessages();

  return (
    <html lang={locale} suppressHydrationWarning>
      <body className="antialiased bg-white dark:bg-[#080b11] text-slate-900 dark:text-slate-100 selection:bg-emerald-500 selection:text-black transition-colors duration-300">
        <NextIntlClientProvider messages={messages}>
          <ThemeProvider>
            <OnboardingTutorial />
            <div className="fixed inset-0 bg-[radial-gradient(ellipse_80%_80%_at_50%_-20%,rgba(16,185,129,0.15),rgba(255,255,255,0))] dark:bg-[radial-gradient(ellipse_80%_80%_at_50%_-20%,rgba(16,185,129,0.15),rgba(255,255,255,0))] pointer-events-none z-0" />
            <div className="relative z-10">
              {children}
            </div>
          </ThemeProvider>
        </NextIntlClientProvider>
      </body>
    </html>
  );
}
