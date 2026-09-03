# PWA Deployment Guide

This document outlines the deployment requirements for the NeuroWealth Vault PWA application.

## Prerequisites
- Application must be served over HTTPS.
- Service worker assets (`sw.js` and `manifest.webmanifest`) must be correctly generated and served at the root.

## Build Process
The PWA is built using `vite-plugin-pwa` within the `packages/vault-ui` package.

Run the following command to build the production application:

```bash
npm run build
```

The build process will automatically generate:
- `manifest.webmanifest`
- Service worker (`sw.js`)
- Necessary PWA icons in the `dist/` directory.

## Deployment Configuration
Ensure your hosting environment (e.g., Vercel, Netlify, or Nginx) is configured to:
1.  Serve static assets from the `packages/vault-ui/dist/` directory.
2.  Properly serve the `manifest.webmanifest` with the `application/manifest+json` MIME type.
3.  Ensure the `sw.js` (service worker) is NOT cached by the server (set `Cache-Control: no-cache` for `sw.js`).
