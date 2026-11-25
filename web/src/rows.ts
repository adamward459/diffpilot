import { diffWordsWithSpace } from 'diff';
import type { DiffLine, FileDiff, LineKind, ReviewThread, Token } from './bridge.ts';

export interface PageFile extends FileDiff {
  collapsed?: boolean;
  viewed?: boolean;
  expanded?: boolean;
  version?: number;
}
export type Range = [start: number, end: number];

interface RowBase {
  f: number;
  key: string;
  h?: number;
}
export type Row =
  | (RowBase & { k: 'fh' | 'gap' | 'load' })
  | (RowBase & { k: 'hk'; text: string })
  | (RowBase & { k: 'big'; reason: string; canLoad?: boolean })
  | (RowBase & { k: 'th' | 'fc'; thread: ReviewThread })
  | LineRow;
export interface LineRow extends RowBase {
  k: 'ln';
  t: LineKind;
  o: number | null;
  n: number | null;
  text: string;
  oi: number;
  ni: number;
  pair?: LineRow;
  ranges?: Range[] | null;
  wrap?: [cols: number, lines: number];
}
export type ClassFor = (light: string, dark: string) => string;

export const SIZE: Record<Row['k'], number> = { fh: 38, hk: 28, ln: 20, gap: 12, big: 96, load: 60, th: 150, fc: 150 };
export const LINE_H = 20;
const TAB_CELLS = 4;
const NOT_PLAIN = /[^\x20-\x7e]/;
export const isPlain = (text: string): boolean => !NOT_PLAIN.test(text);

export function wrapLines(text: string, cols: number): number {
  if (cols < 1) return 1;
  let cells = 0;
  for (let i = 0; i < text.length; i++) cells += text.charCodeAt(i) === 9 ? TAB_CELLS : 1;
  return Math.max(1, Math.ceil(cells / cols));
}

export function lineHeight(r: LineRow, cols: number): number {
  if (r.wrap?.[0] !== cols) r.wrap = [cols, wrapLines(r.text, cols)];
  return r.wrap[1] * LINE_H;
}

export const BIG_LINES = 1500;
export const GENERATED = /(^|\/)(yarn\.lock|package-lock\.json|pnpm-lock\.yaml|Podfile\.lock|.*\.pbxproj|.*\.snap)$/;
const WORD_DIFF_MAX = 500;
const WORD_DIFF_MIN_SAME = 0.4;

const ENTITY: Record<string, string> = { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' };
export const esc = (s: string): string => s.replace(/[&<>"']/g, (c) => ENTITY[c] ?? c);

export function buildFileRows(file: PageFile, f: number, threads: ReviewThread[]): Row[] {
  const out: Row[] = [{ k: 'fh', f, key: `fh${f}` }];
  for (const thread of threads) if (thread.anchor.kind === 'file') out.push({ k: 'fc', f, key: `t${thread.id}`, thread });
  const gap: Row = { k: 'gap', f, key: `g${f}` };
  if (file.collapsed) return [...out, gap];
  if (file.isBinary) return [...out, { k: 'big', f, key: `b${f}`, reason: 'Binary file not shown.' }, gap];
  const lines = file.additions + file.deletions;
  const generated = GENERATED.test(file.path);
  if (!file.expanded && (lines > BIG_LINES || generated)) {
    const reason = generated ? 'Generated file. Hidden by default.' : `Large diff (${lines.toLocaleString('en-US')} lines). Hidden by default.`;
    return [...out, { k: 'big', f, key: `b${f}`, reason, canLoad: true }, gap];
  }
  if (!file.hunks.length) {
    const reason = file.status === 'R' ? 'Renamed without changes.' : 'No content changes.';
    return [...out, { k: 'big', f, key: `b${f}`, reason }, gap];
  }

  const anchored = new Map<string, ReviewThread[]>();
  for (const t of threads) {
    if (t.anchor.kind !== 'line') continue;
    const k = `${t.anchor.side}:${t.anchor.line}`;
    anchored.set(k, [...(anchored.get(k) ?? []), t]);
  }
  let oi = 0;
  let ni = 0;
  file.hunks.forEach((hunk, hi) => {
    out.push({ k: 'hk', f, key: `h${f}:${hi}`, h: hi, text: hunk.header });
    let dels: LineRow[] = [];
    hunk.lines.forEach(([t, o, n, text]: DiffLine, li) => {
      const r: LineRow = { k: 'ln', f, key: `l${f}:${hi}:${li}`, h: hi, t, o, n, text, oi: t !== 'a' ? oi++ : -1, ni: t !== 'd' ? ni++ : -1 };
      if (t === 'd') dels.push(r);
      else if (t === 'a' && dels.length) {
        const d = dels.shift();
        if (d) {
          d.pair = r;
          r.pair = d;
        }
      } else dels = [];
      out.push(r);
      const side = t === 'd' ? 'old' : 'new';
      for (const thread of anchored.get(`${side}:${t === 'd' ? o : n}`) ?? []) out.push({ k: 'th', f, key: `t${thread.id}`, h: hi, thread });
    });
  });
  out.push(gap);
  return out;
}

export function wordRanges(r: LineRow): Range[] | null {
  if (r.ranges !== undefined) return r.ranges;
  const { pair } = r;
  r.ranges = null;
  if (!pair) return null;
  pair.ranges = null;
  const d = r.t === 'd' ? r : pair;
  const a = r.t === 'a' ? r : pair;
  if (d.text.length > WORD_DIFF_MAX || a.text.length > WORD_DIFF_MAX) return null;
  const dr: Range[] = [];
  const ar: Range[] = [];
  let dp = 0;
  let ap = 0;
  let same = 0;
  for (const p of diffWordsWithSpace(d.text, a.text)) {
    const len = p.value.length;
    if (p.removed) {
      dr.push([dp, dp + len]);
      dp += len;
    } else if (p.added) {
      ar.push([ap, ap + len]);
      ap += len;
    } else {
      dp += len;
      ap += len;
      same += len;
    }
  }
  if (same < Math.max(d.text.length, a.text.length) * WORD_DIFF_MIN_SAME) return null;
  d.ranges = dr;
  a.ranges = ar;
  return r.ranges;
}

export function codeHtml(text: string, toks: Token[] | null | undefined, ranges: Range[] | null, classFor: ClassFor = () => ''): string {
  if (!text) return '';
  const cuts = new Set([0, text.length]);
  if (toks) {
    let p = 0;
    for (const [len] of toks) cuts.add((p += len));
  }
  if (ranges) for (const [s, e] of ranges) { cuts.add(s); cuts.add(e); }
  const pts = [...cuts].filter((x) => x <= text.length).sort((a, b) => a - b);
  let out = '';
  let ti = 0;
  let tEnd = toks ? (toks[0]?.[0] ?? text.length) : text.length;
  let ri = 0;
  for (let i = 0; i < pts.length - 1; i++) {
    const s = pts[i] ?? 0;
    const e = pts[i + 1] ?? text.length;
    while (toks && s >= tEnd && ti < toks.length - 1) {
      ti++;
      tEnd += toks[ti]?.[0] ?? 0;
    }
    while (ranges && ri < ranges.length && (ranges[ri]?.[1] ?? 0) <= s) ri++;
    const word = ranges?.[ri];
    const inWord = word !== undefined && word[0] <= s && s < word[1];
    const tok = toks?.[ti];
    const c = tok ? classFor(tok[1], tok[2]) : '';
    const seg = esc(text.slice(s, e));
    out += c || inWord ? `<span class="${c}${inWord ? ' w' : ''}">${seg}</span>` : seg;
  }
  return out;
}
