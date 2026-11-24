import type { Theme } from './bridge.ts';

export interface ThemeTarget {
  style: { setProperty(name: string, value: string): void };
  classList: { toggle(name: string, force: boolean): boolean };
}

export function applyTheme(target: ThemeTarget, theme: Theme): void {
  for (const [name, value] of Object.entries(theme.colors)) target.style.setProperty(name, value);
  target.classList.toggle('dark', theme.isDark);
  target.classList.toggle('contrast', theme.increaseContrast);
  target.classList.toggle('opaque', theme.increaseContrast || theme.reduceTransparency);
}
