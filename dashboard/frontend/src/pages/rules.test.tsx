// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { setUnauthorizedHandler } from '../api/core';
import { navigate, navigationBlocked } from '../router/router';
import type { Route } from '../router/routes';
import Rules from './rules';

/**
 * The acceptance criterion this task is measured against is "a validation
 * failure anchors every message to the right line, and the document is not
 * written" — plus the invariant behind it: the buffer is **byte-identical**
 * after every non-2xx, compared as a string rather than eyeballed.
 */

const ROUTE: Route = {
  path: '/rules',
  title: 'Custom Rules',
  section: 'filtering',
  events: [],
  endpoints: [],
  built: true,
  ownsHeader: true,
  load: null,
};

function respond(status: number, body?: unknown): Response {
  return {
    ok: status >= 200 && status < 300,
    status,
    headers: { get: () => null },
    json: async () => {
      if (body === undefined) throw new Error('no body');
      return body;
    },
  } as unknown as Response;
}

const fetchMock = vi.fn();
let host: HTMLElement | null = null;

/** `request()` awaits `fetch` and then `response.json()`, so a settled mock
 *  still needs several microtask turns before the page has re-rendered. */
async function flush(): Promise<void> {
  await act(async () => {
    for (let turn = 0; turn < 6; turn += 1) await Promise.resolve();
  });
}

async function mount(): Promise<HTMLElement> {
  host = document.createElement('div');
  document.body.append(host);
  await act(async () => {
    render(<Rules route={ROUTE} />, host as HTMLElement);
  });
  await flush();
  return host;
}

async function press(dom: HTMLElement, label: string): Promise<void> {
  await act(async () => {
    button(dom, label).click();
  });
  await flush();
}

function textarea(dom: HTMLElement): HTMLTextAreaElement {
  const found = dom.querySelector('textarea');
  if (found === null) throw new Error('no editor');
  return found;
}

function button(dom: HTMLElement, label: string): HTMLButtonElement {
  const found = [...dom.querySelectorAll('button')].find(
    (candidate) => candidate.textContent?.trim() === label,
  );
  if (found === undefined) throw new Error(`no button: ${label}`);
  return found;
}

async function type(dom: HTMLElement, value: string): Promise<void> {
  const area = textarea(dom);
  await act(async () => {
    area.value = value;
    area.dispatchEvent(new Event('input', { bubbles: true }));
  });
}

const DOCUMENT = ['! personal blocks', '||tracker.example.com^', ''];

beforeEach(() => {
  fetchMock.mockReset();
  vi.stubGlobal('fetch', fetchMock);
  setUnauthorizedHandler(null);
  fetchMock.mockResolvedValue(respond(200, { rules: DOCUMENT }));
});

afterEach(() => {
  if (host !== null) {
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
  vi.unstubAllGlobals();
});

describe('the document round trip', () => {
  it('renders the fetched lines and their count', async () => {
    const dom = await mount();
    expect(textarea(dom).value).toBe(DOCUMENT.join('\n'));
    expect(dom.querySelector('.ch-right')?.textContent).toContain('3 lines');
  });

  // F8 — an empty document is zero lines, not the one empty line `split`
  // reports: a fresh install must read `0 lines`, matching `rules: []`.
  it('reads `0 lines` for an empty document', async () => {
    fetchMock.mockResolvedValue(respond(200, { rules: [] }));
    const dom = await mount();
    expect(textarea(dom).value).toBe('');
    expect(dom.querySelector('.ch-right')?.textContent).toContain('0 lines');
  });

  it('reports the duplicate lines the API dropped, and shows what it stored', async () => {
    const dom = await mount();
    await type(dom, '||a.example^\n||a.example^');
    fetchMock.mockResolvedValue(respond(200, { rules: ['||a.example^'] }));
    await press(dom, 'Validate and save');
    expect(textarea(dom).value).toBe('||a.example^');
    expect(dom.textContent).toContain('1 duplicate line removed');
  });
});

describe('a 422 on the document', () => {
  const REJECTION = {
    error: {
      code: 'validation_failed',
      message:
        'line 2: invalid rule syntax: "@@||^"; line 4: invalid rule syntax: "|||"',
    },
  };

  it('anchors every message to its line and writes nothing', async () => {
    const dom = await mount();
    const typed = '! keep\n@@||^\n\n|||\n';
    await type(dom, typed);
    fetchMock.mockResolvedValue(respond(422, REJECTION));
    await press(dom, 'Validate and save');

    // Byte-identical, trailing blank line included.
    expect(textarea(dom).value).toBe(typed);

    const anchors = [...dom.querySelectorAll('.rule-error-line')].map(
      (node) => node.textContent,
    );
    expect(anchors).toEqual(['line 2', 'line 4']);
    expect(dom.querySelector('.banner.bad')?.textContent).toContain(
      '2 lines failed validation',
    );
    expect(dom.querySelectorAll('.editor-callout')).toHaveLength(2);
    expect(dom.querySelector('.editor-callout')?.textContent).toBe(
      'line 2 — invalid rule syntax: "@@||^"',
    );
  });

  // F15 — a rejection describes the document as it was sent. Measured live:
  // fixing the bad line left the band, the callout and `fix line 2` in place,
  // and the error entry then selected a line that was never rejected.
  it('drops the whole rejection as soon as the text changes', async () => {
    const dom = await mount();
    await type(dom, '! keep\n@@||^\n\n|||\n');
    fetchMock.mockResolvedValue(respond(422, REJECTION));
    await press(dom, 'Validate and save');
    expect(dom.querySelectorAll('.editor-callout')).toHaveLength(2);

    await type(dom, '! keep\n||fixed.example^\n\n|||\n');
    expect(dom.querySelectorAll('.editor-callout')).toHaveLength(0);
    expect(dom.querySelectorAll('.editor-band')).toHaveLength(0);
    expect(dom.querySelectorAll('.rule-error')).toHaveLength(0);
    expect(dom.querySelector('.banner.bad')).toBeNull();
    expect(dom.querySelector('.ch-right')?.textContent).not.toContain(
      'invalid',
    );
  });

  // F16 — the callout shares the bad line's row and starts after that line's
  // text. It used to float over the row beneath, which hid that line entirely.
  it('puts the callout on the offending line, past its own text', async () => {
    const dom = await mount();
    await type(dom, '! keep\n@@||^\n\n|||\n');
    fetchMock.mockResolvedValue(respond(422, REJECTION));
    await press(dom, 'Validate and save');

    const rows = [...dom.querySelectorAll('.editor-callout-row')].map(
      (node) => (node as HTMLElement).style,
    );
    const bands = [...dom.querySelectorAll('.editor-band')].map(
      (node) => (node as HTMLElement).style.top,
    );
    expect(rows.map((style) => style.top)).toEqual(bands);
    // `@@||^` is five columns, so the message starts at the sixth.
    expect(rows[0]?.paddingLeft).toBe('min(6ch + 4px, 60%)');
  });

  it('selects the offending line when its list entry is pressed', async () => {
    const dom = await mount();
    await type(dom, '! keep\n@@||^\n\n|||\n');
    fetchMock.mockResolvedValue(respond(422, REJECTION));
    await press(dom, 'Validate and save');
    const entry = dom.querySelector('.rule-error') as HTMLButtonElement;
    await act(async () => {
      entry.click();
    });
    await flush();
    const area = textarea(dom);
    expect(area.selectionStart).toBe(7);
    expect(area.selectionEnd).toBe(12);
    expect(area.value.slice(7, 12)).toBe('@@||^');
  });

  // The property the whole anchoring design rests on: a format change degrades
  // to an unanchored banner, never to a callout pointing at the wrong line.
  it('renders an unparseable message raw, with zero anchors', async () => {
    const dom = await mount();
    const typed = '||a.example^';
    await type(dom, typed);
    fetchMock.mockResolvedValue(
      respond(422, {
        error: { code: 'validation_failed', message: 'the document was rejected' },
      }),
    );
    await press(dom, 'Validate and save');
    expect(textarea(dom).value).toBe(typed);
    expect(dom.querySelectorAll('.rule-error')).toHaveLength(0);
    expect(dom.querySelectorAll('.editor-callout')).toHaveLength(0);
    expect(dom.querySelector('.banner.bad')?.textContent).toContain(
      'the document was rejected',
    );
    expect(dom.querySelector('.ch-right')?.textContent).not.toContain('invalid');
  });

  it('leaves the buffer untouched on a 500 too', async () => {
    const dom = await mount();
    const typed = 'line one\nline two  \n\n';
    await type(dom, typed);
    fetchMock.mockResolvedValue(
      respond(500, { error: { code: 'internal', message: 'boom' } }),
    );
    await press(dom, 'Validate and save');
    expect(textarea(dom).value).toBe(typed);
    expect(dom.querySelector('.error-state-message')?.textContent).toBe('boom');
  });
});

describe('the blocking save', () => {
  it('blocks navigation and the editor while the compile runs, then releases', async () => {
    const dom = await mount();
    // Saving needs an edit: an unchanged buffer disables the button, so a
    // recompiling PUT cannot be fired for a document nobody touched.
    await type(dom, 'line one\nline two');
    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockReturnValue(
      new Promise<Response>((resolve) => {
        settle = resolve;
      }),
    );
    await press(dom, 'Validate and save');

    expect(dom.querySelector('[aria-busy="true"]')).not.toBeNull();
    expect(navigationBlocked()).toBe(true);
    expect(textarea(dom).disabled).toBe(true);
    expect(button(dom, 'Validate and save').disabled).toBe(true);
    expect(button(dom, 'Discard').disabled).toBe(true);

    // No second PUT can be started, and no route change can unmount the page
    // out from under a request that is never aborted.
    const before = window.location.pathname;
    navigate('/lists');
    expect(window.location.pathname).toBe(before);
    await press(dom, 'Validate and save');
    expect(fetchMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      settle?.(respond(200, { rules: DOCUMENT }));
    });
    await flush();
    expect(dom.querySelector('[aria-busy="true"]')).toBeNull();
    expect(navigationBlocked()).toBe(false);
  });

  // F21 — `Discard` already knew the buffer was clean; `Validate and save` did
  // not, so an untouched document could be posted back and recompile the whole
  // ruleset for nothing.
  it('refuses to save a document nobody edited', async () => {
    const dom = await mount();
    expect(button(dom, 'Validate and save').disabled).toBe(true);
    await type(dom, 'line one');
    expect(button(dom, 'Validate and save').disabled).toBe(false);
  });

  // F18 — the modal's only focusable element is its own root, and
  // `querySelectorAll` never returns the node it is called on: focus stayed on
  // `body` and nothing announced that a blocking compile had started.
  it('moves focus into the busy modal', async () => {
    const dom = await mount();
    await type(dom, 'line one');
    let settle: ((value: Response) => void) | null = null;
    fetchMock.mockReturnValue(
      new Promise<Response>((resolve) => {
        settle = resolve;
      }),
    );
    await press(dom, 'Validate and save');
    expect(document.activeElement).toBe(
      dom.querySelector('[aria-busy="true"]'),
    );
    // The block is module state: leaving it held would fail the next test.
    await act(async () => {
      settle?.(respond(200, { rules: DOCUMENT }));
    });
    await flush();
  });

  it('releases the navigation block when the save fails', async () => {
    const dom = await mount();
    await type(dom, 'line one\nline two');
    fetchMock.mockRejectedValue(new TypeError('offline'));
    await press(dom, 'Validate and save');
    expect(navigationBlocked()).toBe(false);
    expect(dom.textContent).toContain('did not reach the server');
  });
});

describe('Discard', () => {
  it('is inert until the buffer differs, and confirms before throwing text away', async () => {
    const dom = await mount();
    expect(button(dom, 'Discard').disabled).toBe(true);
    await type(dom, 'something else');
    expect(button(dom, 'Discard').disabled).toBe(false);

    await press(dom, 'Discard');
    expect(dom.querySelector('.dialog')?.textContent).toContain(
      'Discard your changes?',
    );
    const confirm = dom.querySelector(
      '.dialog-actions .btn:not(.g)',
    ) as HTMLButtonElement;
    expect(confirm.textContent).toBe('Discard');
    await act(async () => {
      confirm.click();
    });
    await flush();
    expect(textarea(dom).value).toBe(DOCUMENT.join('\n'));
  });
});
