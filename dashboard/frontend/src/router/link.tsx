import type { ComponentChildren, JSX } from 'preact';
import { isPlainLeftClick, navigate } from './router';

interface LinkProps
  extends Omit<JSX.HTMLAttributes<HTMLAnchorElement>, 'href' | 'onClick'> {
  href: string;
  children?: ComponentChildren;
}

/**
 * A real `<a href>`: middle-click, ⌘-click and "open in new tab" keep working,
 * and only a plain left-click is taken over by the router.
 */
export function Link({ href, children, ...rest }: LinkProps) {
  return (
    <a
      href={href}
      onClick={(event: MouseEvent) => {
        if (!isPlainLeftClick(event)) return;
        event.preventDefault();
        navigate(href);
      }}
      {...rest}
    >
      {children}
    </a>
  );
}
