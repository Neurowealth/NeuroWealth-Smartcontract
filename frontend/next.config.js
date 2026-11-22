/** @type {import('next').NextConfig} */
const createNextIntlPlugin = require('next-intl/plugin');

const withNextIntl = createNextIntlPlugin(
  './src/i18n/request.ts'
);

const nextConfig = {
  reactStrictMode: true,
  transpilePackages: ['@stellar/stellar-sdk', '@stellar/freighter-api'],
};

module.exports = withNextIntl(nextConfig);
