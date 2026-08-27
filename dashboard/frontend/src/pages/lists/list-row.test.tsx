// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, describe, expect, it } from 'vitest';
import type { ListItem, ListStatus } from '../../api/types';
import { ListRow } from './list-row';
import { ListActions } from './list-actions';

/**
 * `Lists.dc.html` and `MobileLists.dc.html` are the source of truth for this
 * row's structure, and until now nothing rendered it: every defect found in the
 * p5-06 review that a machine could have caught lived in the gap between the
 * artboards and a component no test mounted.
 *
 * These assert cell inventory, cell order and the status vocabulary — not
 * geometry, which jsdom cannot resolve and `styles/grid-tracks.test.ts` covers
 * from the stylesheet instead.
 */

const NOW = Date.parse('2026-08-27T12:00:00Z');

function item(over: Partial<ListItem> = {}): ListItem {
  return {
    id: 'oisd-basic',
    url: 'https://small.oisd.nl',
    format: 'auto',
    enabled: true,
    refresh_hours: 24,
    last_refresh: '2026-08-27T04:00:00Z',
    last_status: 'ok',
    rules_total: 223182,
    rules_active_dns: 198500,
    rules_active_url: 9181,
    rules_inactive: 15501,
    parse_errors: 0,
    ...over,
  };
}

let host: HTMLElement | null = null;

function mount(node: preact.ComponentChild): HTMLElement {
  host = document.createElement('div');
  document.body.append(host);
  act(() => {
    render(node, host as HTMLElement);
  });
  return host;
}

afterEach(() => {
  if (host !== null) {
    render(null, host);
    host.remove();
    host = null;
  }
});

function row(
  over: Partial<ListItem> = {},
  { pending = false }: { pending?: boolean } = {},
): HTMLElement {
  return mount(
    <ListRow
      item={item(over)}
      now={NOW}
      actions={
        <ListActions
          item={item(over)}
          pending={pending}
          onRefresh={() => undefined}
          onEdit={() => undefined}
          onRemove={() => undefined}
          onReadd={() => undefined}
        />
      }
    />,
  );
}

describe('the row', () => {
  it('lays its cells out in the artboard’s column order', () => {
    const cells = Array.from(
      row().querySelectorAll('.list-row > *'),
    ).map((cell) => cell.className.split(' ')[0]);
    // List · (On · Every · Last refresh, grouped so the phone can make them one
    // 44 px row) · Status · Rules · Total · actions.
    expect(cells).toEqual([
      'l-id',
      'l-meta',
      'l-status',
      'l-rules',
      'l-total',
      'l-actions',
    ]);
  });

  it('prints the id and its source', () => {
    const cell = row().querySelector('.l-id');
    expect(cell?.querySelector('.l-name')?.textContent).toBe('oisd-basic');
    expect(cell?.querySelector('.l-source')?.textContent).toBe(
      'https://small.oisd.nl',
    );
  });

  it('prints the partition and the parse errors beside it', () => {
    const partition = row().querySelector('.l-partition')?.textContent;
    expect(partition).toContain('198,500 dns');
    expect(partition).toContain('9,181 url');
    expect(partition).toContain('15,501 inactive');
    expect(partition).toContain('0 parse errors');
  });

  it('prints the total from the field, not from the three parts', () => {
    // The three counts partition `rules_total`, but the column is the served
    // figure — summing them here would invent an identity the API does not
    // promise.
    expect(row().querySelector('.l-total')?.textContent).toBe('223,182');
  });

  it('shows the interval only while the list is enabled', () => {
    expect(row().querySelector('.l-every')?.textContent).toBe('24 h');
    expect(row({ enabled: false }).querySelector('.l-every')?.textContent).toBe(
      '—',
    );
  });

  it('says `never` for a list not yet refreshed in this process', () => {
    expect(
      row({ last_refresh: null }).querySelector('.l-last')?.textContent,
    ).toBe('never');
  });
});

describe('the five statuses, plus disabled', () => {
  const cases: Array<[ListStatus, string]> = [
    ['ok', 'ok'],
    ['degraded', 'degraded'],
    ['failed', 'failed'],
    ['rejected', 'rejected'],
    ['never', 'never'],
  ];

  it.each(cases)('renders %s as its own pill', (status, word) => {
    const pill = row({ last_status: status, last_error: 'why' }).querySelector(
      '.l-status .pill',
    );
    expect(pill?.textContent).toBe(word);
  });

  it('never renders `degraded` as a milder `ok`', () => {
    const degraded = row({ last_status: 'degraded' });
    expect(degraded.querySelector('.pill')?.className).not.toContain('good');
    expect(degraded.querySelector('.l-status')?.textContent).toContain(
      'failed to parse',
    );
    expect(degraded.querySelector('.l-status')?.textContent).toContain(
      'RULE_ENGINE.md',
    );
  });

  it('says the last good copy is still serving on a failure', () => {
    const failed = row({ last_status: 'failed', last_error: 'fetch failed: x' });
    expect(failed.querySelector('.l-status')?.textContent).toContain(
      'fetch failed: x',
    );
    expect(failed.querySelector('.l-status')?.textContent).toContain(
      'last good copy still serving',
    );
  });

  it('explains a rejection as the content gate', () => {
    const rejected = row({
      last_status: 'rejected',
      last_error: 'rejected: html document',
    });
    expect(rejected.querySelector('.l-status')?.textContent).toContain(
      'rejected: html document',
    );
    expect(rejected.querySelector('.l-status')?.textContent).toContain(
      'content gate refused the body',
    );
  });

  it('shows DISABLED with the real last attempt beneath it', () => {
    // `DISABLED` alone would make a list that failed look clean.
    const disabled = row({ enabled: false, last_status: 'failed' });
    expect(disabled.querySelector('.pill')?.textContent).toBe('disabled');
    expect(disabled.querySelector('.list-status-secondary')?.textContent).toBe(
      'last attempt: failed',
    );
  });

  it('omits the secondary line when the disabled list was never fetched', () => {
    const disabled = row({ enabled: false, last_status: 'never' });
    expect(disabled.querySelector('.list-status-secondary')).toBeNull();
  });

  it('tints the row for a live failure and not for a disabled one', () => {
    expect(row({ last_status: 'failed' }).querySelector('.is-alert')).not.toBeNull();
    expect(
      row({ enabled: false, last_status: 'failed' }).querySelector('.is-alert'),
    ).toBeNull();
  });

  it('does not compile a disabled list', () => {
    const disabled = row({ enabled: false });
    expect(disabled.querySelector('.l-partition')?.textContent).toBe(
      'not compiled while disabled',
    );
  });
});

describe('the row actions', () => {
  /**
   * They are glyphs, so the word lives in `aria-label` and `title` rather than
   * in the button's text. That is the whole accessible name — a test that reads
   * `textContent` here would pass on an empty button, which is exactly the
   * failure mode icon actions have.
   */
  const named = (el: Element) =>
    Array.from(el.querySelectorAll('.row-actions button')).map((button) => ({
      label: button.getAttribute('aria-label'),
      title: button.getAttribute('title'),
      glyph: button
        .querySelector('use')
        ?.getAttribute('href')
        ?.split('#')
        .pop(),
    }));

  it('offers refresh, edit and remove in that order', () => {
    expect(named(row())).toEqual([
      { label: 'Refresh oisd-basic', title: 'Refresh', glyph: 'refresh' },
      { label: 'Edit oisd-basic', title: 'Edit', glyph: 'edit' },
      { label: 'Remove oisd-basic', title: 'Remove', glyph: 'trash' },
    ]);
  });

  it('names every action, so no button is a bare glyph', () => {
    for (const action of named(row())) {
      expect(action.label).toBeTruthy();
      expect(action.title).toBeTruthy();
      expect(action.glyph).toBeTruthy();
    }
  });

  it('replaces Refresh with delete-and-re-add on a rejected list', () => {
    // Neither a refresh nor a disable/enable clears the cached copy the content
    // gate measures against, so refresh is the one action that cannot work.
    const actions = named(row({ last_status: 'rejected' }));
    expect(actions[0]).toEqual({
      label: 'Delete and re-add oisd-basic',
      title: 'Delete and re-add',
      glyph: 'restore',
    });
    // A distinct glyph, not the refresh arrow reused: the two actions differ in
    // blast radius and must not look the same.
    expect(actions[0]?.glyph).not.toBe('refresh');
    expect(actions.slice(1).map((a) => a.title)).toEqual(['Edit', 'Remove']);
  });

  it('says so in the accessible name while a refresh is pending', () => {
    // The pending state is the same button in a busy state — no second label,
    // so it costs no width and cannot move a column. The fact still has to
    // reach a screen reader, which is what this pins.
    const pending = row({}, { pending: true });
    const refresh = pending.querySelector('.row-actions button');
    expect(refresh?.getAttribute('aria-label')).toBe(
      'Refresh requested for oisd-basic',
    );
    expect(refresh?.className).toContain('is-busy');
    expect((refresh as HTMLButtonElement).disabled).toBe(true);
  });

  it('cannot refresh a disabled list', () => {
    const refresh = row({ enabled: false }).querySelector('.row-actions button');
    expect((refresh as HTMLButtonElement).disabled).toBe(true);
  });
});
