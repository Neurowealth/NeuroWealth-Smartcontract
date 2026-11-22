'use client';

import { usePathname, useRouter } from 'next/navigation';
import { useLocale, useTranslations } from 'next-intl';

export function LanguageSwitcher() {
  const t = useTranslations('Index');
  const locale = useLocale();
  const router = useRouter();
  const pathname = usePathname();

  const handleLanguageChange = (e: React.ChangeEvent<HTMLSelectElement>) => {
    const nextLocale = e.target.value;
    const currentPathname = pathname.replace(`/${locale}`, '');
    router.replace(`/${nextLocale}${currentPathname}`);
  };

  return (
    <div className="flex items-center gap-2">
      <label htmlFor="language-select" className="sr-only">
        {t('language')}
      </label>
      <select
        id="language-select"
        value={locale}
        onChange={handleLanguageChange}
        className="bg-transparent text-sm border-none focus:ring-0 dark:text-white"
      >
        <option value="en">English</option>
        <option value="es">Español</option>
        <option value="fr">Français</option>
        <option value="pt">Português</option>
        <option value="zh">中文</option>
        <option value="ja">日本語</option>
      </select>
    </div>
  );
}
