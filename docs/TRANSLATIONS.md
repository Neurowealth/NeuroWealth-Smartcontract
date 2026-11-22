# Translation Contribution Guide

NeuroWealth uses `next-intl` for internationalization. We support multiple languages to serve a global audience.

## Adding a New Language

1. Update the `locales` array in `frontend/src/i18n/request.ts` and `frontend/src/middleware.ts`.
2. Create a new JSON file in `frontend/messages/` named with the locale code (e.g., `de.json` for German).
3. Copy the keys from `en.json` and translate the values.
4. Update the `LanguageSwitcher` component in `frontend/src/components/LanguageSwitcher.tsx` to include the new language.

## Updating Existing Translations

Simply edit the relevant JSON file in `frontend/messages/`. The structure must match `en.json`. If a key is missing in another language, it will fallback to English automatically if configured or display the key.
