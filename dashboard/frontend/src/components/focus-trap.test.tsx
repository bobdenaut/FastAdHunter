// @vitest-environment jsdom
import { render } from 'preact';
import { useRef, useState } from 'preact/hooks';
import { act } from 'preact/test-utils';
import { afterEach, describe, expect, it } from 'vitest';
import { useFocusTrap } from './focus-trap';

let host: HTMLElement | null = null;

function Trapped({ onEscape }: { onEscape: () => void }) {
  const box = useRef<HTMLDivElement>(null);
  const [active, setActive] = useState(true);
  useFocusTrap(active, () => box.current, () => {
    setActive(false);
    onEscape();
  });
  return (
    <div>
      <button type="button" id="invoker">
        open
      </button>
      {active && (
        <div ref={box}>
          <button type="button" id="first">
            first
          </button>
          <button type="button" id="last">
            last
          </button>
        </div>
      )}
    </div>
  );
}

function key(init: KeyboardEventInit): void {
  document.dispatchEvent(
    new KeyboardEvent('keydown', { bubbles: true, cancelable: true, ...init }),
  );
}

function mount(node: preact.ComponentChild): void {
  act(() => {
    host = document.createElement('div');
    document.body.append(host);
    render(node, host);
  });
}

afterEach(() => {
  if (host !== null) {
    render(null, host);
    host.remove();
  }
  host = null;
});

describe('the shared focus trap', () => {
  it('moves focus into the trapped region', () => {
    mount(<Trapped onEscape={() => {}} />);
    expect(document.activeElement?.id).toBe('first');
  });

  it('cycles Tab at the end and Shift+Tab at the start', () => {
    mount(<Trapped onEscape={() => {}} />);
    document.getElementById('last')?.focus();
    key({ key: 'Tab' });
    expect(document.activeElement?.id).toBe('first');
    key({ key: 'Tab', shiftKey: true });
    expect(document.activeElement?.id).toBe('last');
  });

  it('asks the owner to close on Escape and returns focus to the invoker', () => {
    let escapes = 0;
    mount(<Trapped onEscape={() => { escapes += 1; }} />);
    const invoker = document.getElementById('invoker') as HTMLElement;
    invoker.focus();
    act(() => { key({ key: 'Escape' }); });
    expect(escapes).toBe(1);
    expect(document.activeElement).toBe(invoker);
  });

  it('adds no listener while inactive', () => {
    let escapes = 0;
    function Inactive() {
      useFocusTrap(false, () => null, () => { escapes += 1; });
      return <button type="button" id="only">only</button>;
    }
    mount(<Inactive />);
    key({ key: 'Escape' });
    expect(escapes).toBe(0);
  });
});
