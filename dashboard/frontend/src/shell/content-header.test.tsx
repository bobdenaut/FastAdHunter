// @vitest-environment jsdom
import { render } from 'preact';
import { act } from 'preact/test-utils';
import { afterEach, describe, expect, it } from 'vitest';
import { ROUTES } from '../router/routes';
import { ContentHeader } from './content-header';

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
    act(() => {
      render(null, host as HTMLElement);
    });
    host.remove();
    host = null;
  }
});

describe('the page header seam', () => {
  it('renders a title alone exactly as it did before the slot existed', () => {
    const dom = mount(<ContentHeader title="Dashboard" />);
    expect(dom.innerHTML).toBe(
      '<div class="hd"><div><p class="h1">Dashboard</p></div></div>',
    );
  });

  it('renders context, cluster and actions when a page fills them', () => {
    const dom = mount(
      <ContentHeader
        title="Custom rules"
        context="your own rules"
        actions={<button type="button">Save</button>}
      />,
    );
    expect(dom.querySelector('.sub')?.textContent).toBe('your own rules');
    expect(dom.querySelector('.hd-actions button')?.textContent).toBe('Save');
  });
});

describe('who owns the header', () => {
  // The shell skips its own header only for a route that says so. Everything
  // p5-06 shipped keeps the shell's, which is what makes the seam additive.
  it('is the shell for every route that does not declare otherwise', () => {
    const owning = ROUTES.filter((route) => route.ownsHeader === true).map(
      (route) => route.path,
    );
    for (const path of ['/', '/lists']) {
      expect(owning).not.toContain(path);
    }
  });
});
