// Public entry point for @neurowealth/vault-ui (referenced by package.json
// "main"/"types"). See docs/FRONTEND_PACKAGES.md: frontend/ is the canonical
// app; this package only ships reusable components and notification logic.
export { default as DepositWithdrawModal } from './components/DepositWithdrawModal';
export { default as EarningsHistoryPage } from './components/EarningsHistoryPage';
export { default as NotificationSettings } from './components/NotificationSettings';
export * from './notifications';
