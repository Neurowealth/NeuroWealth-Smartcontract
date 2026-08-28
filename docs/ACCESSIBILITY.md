# Accessibility Statement

> **Issue:** #668
> **Standard:** [WCAG 2.1 Level AA](https://www.w3.org/TR/WCAG21/)
> **Scope:** `@neurowealth/vault-ui` (`packages/vault-ui`)

NeuroWealth is committed to making the vault dashboard usable by people with disabilities. This statement describes conformance, known limitations, and how we test.

## Conformance

The vault UI targets **WCAG 2.1 Level AA**. Automated checks (axe-core) run in CI on every pull request. Manual screen-reader passes (VoiceOver on macOS/iOS, NVDA on Windows) are required before a UI release and are recorded in the checklist below.

## What is in place

| Criterion | Implementation |
|-----------|----------------|
| Keyboard access | Every control is a native `button`, `a`, `input`, `select`, or `checkbox`. Tab order follows visual order. |
| Skip navigation | “Skip to main content” link is the first focusable element and jumps to `#main-content`. |
| Screen readers | Landmarks (`header`, `nav`, `main`), `aria-label` / `aria-labelledby`, `aria-live` for status and errors, `aria-invalid` + `aria-describedby` on the amount field. |
| Focus indicators | `:focus-visible` outline uses `primary-700` (`#1d4ed8`) at 0.125rem, offset 0.125rem. |
| Colour contrast | Body and UI text use `gray-900` / `gray-700` on `gray-50`/`white` (≥ 4.5:1). Primary actions use `primary-700` on white (≥ 4.5:1). |
| Text sizing | Layout and type use rem-based Tailwind tokens; the root font size is not locked to pixels. Users can zoom to 200% without loss of function. |
| Images / icons | Decorative marks are CSS-only. Informative images have `alt`. Charts expose a visually hidden data table. |
| Forms | Every input has a `<label>`. Errors are announced via `role="alert"` and referenced from the field. |
| Reduced motion | `prefers-reduced-motion: reduce` disables non-essential transitions. |

## Automated testing

```bash
cd packages/vault-ui
npm test
```

`src/a11y.test.tsx` renders the app (including deposit/withdraw, earnings, and notification preferences) and runs [axe-core](https://github.com/dequelabs/axe-core) via `vitest-axe` with WCAG 2.1 AA tags. The `vault-ui-test` CI job fails the PR on violations.

jsdom cannot fully evaluate colour contrast or screen-reader output, which is why the manual checklist exists.

## Manual screen-reader checklist

Run before tagging a UI release. Tick each item in the PR description.

### VoiceOver (macOS)

- [ ] Rotor lists Headings, Landmarks, and Form Controls for Deposit / Withdraw, Earnings, and Notifications.
- [ ] Skip link is the first VoiceOver landing and moves focus into `<main>`.
- [ ] Amount field label, preview, and error text are announced together.
- [ ] Success and error toasts are spoken when they appear (`aria-live`).
- [ ] Chart regions announce their `aria-label`; the data table is reachable.

### NVDA (Windows, Firefox or Chrome)

- [ ] Same landmark / heading structure as VoiceOver.
- [ ] Focus is visible while tabbing with NVDA off (keyboard-only).
- [ ] Checkbox group in notification preferences is read as a group (`fieldset` / `legend`).
- [ ] Push-permission and email-fallback status changes are announced.

### Keyboard-only

- [ ] Tab, Shift+Tab, Enter, and Space operate every control.
- [ ] No keyboard trap in the deposit modal or notification panel.
- [ ] Esc is not required to complete a task (there is no blocking overlay).

## Feedback

Accessibility bugs can be filed against this repository with the `a11y` label, or reported through the process in [`docs/BUG_BOUNTY.md`](BUG_BOUNTY.md) when they affect fund operations.
