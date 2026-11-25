import assert from 'node:assert/strict';
import { readFileSync } from 'node:fs';
import { test } from 'node:test';
import { SYMBOLS } from '../src/icons.ts';
import { applyTheme } from '../src/theme.ts';
import type { ThemeTarget } from '../src/theme.ts';

const fakeTarget = () => {
  const vars = new Map<string, string>();
  const classes = new Set<string>();
  const target: ThemeTarget = {
    style: { setProperty: (name, value) => void vars.set(name, value) },
    classList: {
      toggle: (name, force) => {
        if (force) classes.add(name);
        else classes.delete(name);
        return force;
      },
    },
  };
  return { target, vars, classes };
};

test('applyTheme sets every host color as a CSS variable', () => {
  const { target, vars } = fakeTarget();
  applyTheme(target, {
    colors: { '--text': 'rgb(0 0 0 / 0.85)', '--accent': 'rgb(0 122 255 / 1)' },
    isDark: false,
    increaseContrast: false,
    reduceTransparency: false,
  });
  assert.equal(vars.get('--text'), 'rgb(0 0 0 / 0.85)');
  assert.equal(vars.get('--accent'), 'rgb(0 122 255 / 1)');
});

test('applyTheme follows the dark, contrast and transparency flags and clears them again', () => {
  const { target, classes } = fakeTarget();
  applyTheme(target, { colors: {}, isDark: true, increaseContrast: false, reduceTransparency: true });
  assert.deepEqual([...classes].sort(), ['dark', 'opaque']);
  applyTheme(target, { colors: {}, isDark: false, increaseContrast: true, reduceTransparency: false });
  assert.deepEqual([...classes].sort(), ['contrast', 'opaque']);
  applyTheme(target, { colors: {}, isDark: false, increaseContrast: false, reduceTransparency: false });
  assert.deepEqual([...classes], []);
});

test('every page icon is a symbol that Swift can render', () => {
  const swift = readFileSync(new URL('../../diffpilot/Features/DiffPane/PaneSymbols.swift', import.meta.url), 'utf8');
  for (const symbol of Object.values(SYMBOLS)) {
    assert.ok(swift.includes(`"${symbol}"`) || new RegExp(`case ${symbol}\\b`).test(swift), `${symbol} is not in PaneSymbol`);
  }
});
