import { diffWordsWithSpace } from 'diff';

export const SIZE = { fh: 38, hk: 28, ln: 20, gap: 12, big: 96, load: 60, th: 150, fc: 150 };
export const BIG_LINES = 1500;
export const GENERATED = /(^|\/)(yarn\.lock|package-lock\.json|pnpm-lock\.yaml|Podfile\.lock|.*\.pbxproj|.*\.snap)$/;
const WORD_DIFF_MAX = 500;
const WORD_DIFF_MIN_SAME = 0.4;

export const esc = (s) => s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');

export function buildFileRows(file, f, threads) {
  const out = [{ k: 'fh', f, key: `fh${f}` }];
  for (const t of threads) if (t.anchor.kind === 'file') out.push({ k: 'fc', f, key: `t${t.id}`, t });
  const gap = { k: 'gap', f, key: `g${f}` };
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

  const anchored = new Map();
  for (const t of threads) {
    if (t.anchor.kind !== 'line') continue;
    const k = `${t.anchor.side}:${t.anchor.line}`;
    anchored.set(k, [...(anchored.get(k) || []), t]);
  }
  let oi = 0;
  let ni = 0;
  file.hunks.forEach((hunk, hi) => {
    out.push({ k: 'hk', f, key: `h${f}:${hi}`, text: hunk.header });
    let dels = [];
    hunk.lines.forEach(([t, o, n, text], li) => {
      const r = { k: 'ln', f, key: `l${f}:${hi}:${li}`, t, o, n, text, oi: t !== 'a' ? oi++ : -1, ni: t !== 'd' ? ni++ : -1 };
      if (t === 'd') dels.push(r);
      else if (t === 'a' && dels.length) {
        const d = dels.shift();
        d.pair = r;
        r.pair = d;
      } else dels = [];
      out.push(r);
      const side = t === 'd' ? 'old' : 'new';
      for (const th of anchored.get(`${side}:${t === 'd' ? o : n}`) || []) out.push({ k: 'th', f, key: `t${th.id}`, t: th });
    });
  });
  out.push(gap);
  return out;
}

export function wordRanges(r) {
  if (r.ranges !== undefined) return r.ranges;
  r.ranges = r.pair.ranges = null;
  const d = r.t === 'd' ? r : r.pair;
  const a = r.t === 'a' ? r : r.pair;
  if (d.text.length > WORD_DIFF_MAX || a.text.length > WORD_DIFF_MAX) return null;
  const dr = [];
  const ar = [];
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

export function codeHtml(text, toks, ranges, classFor = () => '') {
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
  let tEnd = toks ? toks[0]?.[0] ?? text.length : text.length;
  let ri = 0;
  for (let i = 0; i < pts.length - 1; i++) {
    const s = pts[i];
    const e = pts[i + 1];
    while (toks && s >= tEnd && ti < toks.length - 1) {
      ti++;
      tEnd += toks[ti][0];
    }
    while (ranges && ri < ranges.length && ranges[ri][1] <= s) ri++;
    const inWord = ranges && ri < ranges.length && ranges[ri][0] <= s && s < ranges[ri][1];
    const c = toks && toks[ti] ? classFor(toks[ti][1], toks[ti][2]) : '';
    const seg = esc(text.slice(s, e));
    out += c || inWord ? `<span class="${c}${inWord ? ' w' : ''}">${seg}</span>` : seg;
  }
  return out;
}
