import { render } from 'preact';
import './styles/tokens.css';
import './styles/base.css';
import './styles/layout.css';
import './styles/components.css';
import { App } from './app';
import { initTheme } from './theme/theme';

initTheme();

const root = document.getElementById('app');
if (root !== null) {
  render(<App />, root);
}
