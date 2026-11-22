import { beforeEach, describe, expect, it, vi } from 'vitest';

const { render, createRoot } = vi.hoisted(() => {
  const render = vi.fn();
  return { render, createRoot: vi.fn(() => ({ render })) };
});

vi.mock('react-dom/client', () => ({
  default: { createRoot },
  createRoot,
}));

vi.mock('./App', () => ({
  default: () => null,
}));

vi.mock('./index.css', () => ({}));

describe('main entrypoint', () => {
  beforeEach(() => {
    vi.resetModules();
    createRoot.mockClear();
    render.mockClear();
    document.body.innerHTML = '<div id="root"></div>';
  });

  it('mounts the app exactly once on the root element', async () => {
    await import('./main');

    expect(createRoot).toHaveBeenCalledTimes(1);
    expect(createRoot).toHaveBeenCalledWith(document.getElementById('root'));
    expect(render).toHaveBeenCalledTimes(1);
  });
});
