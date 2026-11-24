import { test } from 'node:test';
import assert from 'node:assert/strict';
import { BIG_LINES, buildFileRows, codeHtml, wordRanges } from '../src/rows.js';

const file = (over = {}) => ({
  path: 'src/a.ts', status: 'M', isBinary: false, additions: 2, deletions: 1,
  hunks: [{ header: '@@ -1,2 +1,3 @@', lines: [['c', 1, 1, 'keep'], ['d', 2, null, 'old line'], ['a', null, 2, 'new line'], ['a', null, 3, 'extra']] }],
  ...over,
});
const kinds = (rows) => rows.map((r) => r.k);

test('rows run header, hunk, lines, gap', () => {
  const rows = buildFileRows(file(), 0, []);
  assert.deepEqual(kinds(rows), ['fh', 'hk', 'ln', 'ln', 'ln', 'ln', 'gap']);
  assert.equal(rows[1].text, '@@ -1,2 +1,3 @@');
});

test('row keys are unique and carry the file index', () => {
  const rows = [...buildFileRows(file(), 3, []), ...buildFileRows(file(), 4, [])];
  assert.equal(new Set(rows.map((r) => r.key)).size, rows.length);
  assert.ok(rows.every((r) => r.f === 3 || r.f === 4));
});

test('token indexes count each side on its own', () => {
  const lines = buildFileRows(file(), 0, []).filter((r) => r.k === 'ln');
  assert.deepEqual(lines.map((r) => [r.oi, r.ni]), [[0, 0], [1, -1], [-1, 1], [-1, 2]]);
});

test('token indexes continue across hunks', () => {
  const f = file({ hunks: [
    { header: 'h1', lines: [['c', 1, 1, 'a']] },
    { header: 'h2', lines: [['c', 9, 9, 'b']] },
  ] });
  const lines = buildFileRows(f, 0, []).filter((r) => r.k === 'ln');
  assert.deepEqual(lines.map((r) => [r.oi, r.ni]), [[0, 0], [1, 1]]);
});

test('a deleted line pairs with the added line that follows it', () => {
  const lines = buildFileRows(file(), 0, []).filter((r) => r.k === 'ln');
  assert.equal(lines[1].pair, lines[2]);
  assert.equal(lines[2].pair, lines[1]);
  assert.equal(lines[3].pair, undefined);
});

test('line threads follow their line, file threads follow the header', () => {
  const lineThread = { id: 7, anchor: { kind: 'line', path: 'src/a.ts', side: 'new', line: 2 }, comments: [] };
  const oldThread = { id: 8, anchor: { kind: 'line', path: 'src/a.ts', side: 'old', line: 2 }, comments: [] };
  const fileThread = { id: 9, anchor: { kind: 'file', path: 'src/a.ts' }, comments: [] };
  const rows = buildFileRows(file(), 0, [lineThread, oldThread, fileThread]);
  assert.deepEqual(kinds(rows), ['fh', 'fc', 'hk', 'ln', 'ln', 'th', 'ln', 'th', 'ln', 'gap']);
  const at = (k, id) => rows.findIndex((r) => r.k === k && r.t?.id === id);
  assert.equal(rows[at('th', 8) - 1].o, 2);
  assert.equal(rows[at('th', 7) - 1].n, 2);
  assert.equal(at('fc', 9), 1);
});

test('a collapsed file shows only its header, file threads and gap', () => {
  const t = { id: 1, anchor: { kind: 'file', path: 'src/a.ts' }, comments: [] };
  assert.deepEqual(kinds(buildFileRows(file({ collapsed: true }), 0, [t])), ['fh', 'fc', 'gap']);
});

test('a large diff is hidden until loaded', () => {
  const big = file({ additions: BIG_LINES + 1 });
  const hidden = buildFileRows(big, 0, []);
  assert.deepEqual(kinds(hidden), ['fh', 'big', 'gap']);
  assert.equal(hidden[1].canLoad, true);
  assert.ok(kinds(buildFileRows({ ...big, expanded: true }, 0, [])).includes('ln'));
});

test('lock files are hidden as generated', () => {
  const rows = buildFileRows(file({ path: 'pnpm-lock.yaml' }), 0, []);
  assert.match(rows[1].reason, /Generated/);
});

test('binary files show a note and no lines', () => {
  const rows = buildFileRows(file({ isBinary: true, hunks: [] }), 0, []);
  assert.deepEqual(kinds(rows), ['fh', 'big', 'gap']);
  assert.match(rows[1].reason, /Binary/);
  assert.equal(rows[1].canLoad, undefined);
});

test('a pure rename shows a note', () => {
  const rows = buildFileRows(file({ status: 'R', oldPath: 'old.ts', hunks: [] }), 0, []);
  assert.match(rows[1].reason, /Renamed without changes/);
});

test('wordRanges marks the changed words on both sides', () => {
  const [, , , d, a] = buildFileRows(file(), 0, []);
  const dr = wordRanges(d);
  assert.deepEqual(dr, [[0, 3]]);
  assert.deepEqual(a.ranges, [[0, 3]]);
  assert.equal(wordRanges(a), a.ranges);
});

test('wordRanges skips pairs that share too little', () => {
  const f = file({ hunks: [{ header: 'h', lines: [['d', 1, null, 'aaaaaaaaaa'], ['a', null, 1, 'bbbbbbbbbb']] }] });
  const [, , d, a] = buildFileRows(f, 0, []);
  assert.equal(wordRanges(d), null);
  assert.equal(a.ranges, null);
});

test('wordRanges skips very long lines', () => {
  const long = 'x '.repeat(300);
  const f = file({ hunks: [{ header: 'h', lines: [['d', 1, null, long], ['a', null, 1, long + 'y']] }] });
  const [, , d] = buildFileRows(f, 0, []);
  assert.equal(wordRanges(d), null);
});

test('codeHtml escapes markup', () => {
  assert.equal(codeHtml('a<b>&c', null, null), 'a&lt;b&gt;&amp;c');
  assert.equal(codeHtml('', null, null), '');
});

test('codeHtml wraps each token with its class', () => {
  const html = codeHtml('let x', [[3, '#a', '#b'], [2, '#c', '#d']], null, (l, d) => `c${l}${d}`);
  assert.equal(html, '<span class="c#a#b">let</span><span class="c#c#d"> x</span>');
});

test('codeHtml marks word ranges, also inside a token', () => {
  const html = codeHtml('abcdef', [[6, '#a', '#b']], [[2, 4]], () => 'k');
  assert.equal(html, '<span class="k">ab</span><span class="k w">cd</span><span class="k">ef</span>');
});

test('codeHtml marks word ranges without tokens', () => {
  assert.equal(codeHtml('abcd', null, [[1, 2]]), 'a<span class=" w">b</span>cd');
});
