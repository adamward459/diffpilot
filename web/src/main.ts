import { Virtualizer, elementScroll, measureElement, observeElementOffset, observeElementRect } from '@tanstack/virtual-core';
import type { VirtualItem } from '@tanstack/virtual-core';
import type { FileDiff, HighlightRequest, HighlightResult, HostApi, ReviewThread, Token } from './bridge.ts';
import { send } from './bridge.ts';
import { I, fileIcon } from './icons.ts';
import { SIZE, buildFileRows, codeHtml, esc, wordRanges } from './rows.ts';
import type { PageFile, Row } from './rows.ts';
import { mountThread, unmountThread } from './threads.tsx';
import type { ThreadApi } from './threads.tsx';
import { btn, dots } from './ui.ts';

interface Tokens {
  old: Token[][];
  new: Token[][];
}
interface Mounted {
  el: HTMLDivElement;
  row: Row;
  y?: number;
  index?: number;
  inner?: HTMLElement;
}

const MEASURED = new Set<Row['k']>(['th', 'fc']);

const S = {
  files: [] as PageFile[],
  fileRows: [] as Row[][],
  rows: [] as Row[],
  done: false,
  pending: false,
  inset: 0,
  topFile: -1,
  threads: new Map<number, ReviewThread>(),
  byFile: new Map<string, ReviewThread[]>(),
  tokens: new Map<number, Tokens>(),
  hlQueued: new Set<number>(),
  nodes: new Map<string, Mounted>(),
  threadEls: new Map<number, HTMLElement>(),
};

const byId = <T extends HTMLElement>(id: string): T => {
  const el = document.getElementById(id);
  if (!el) throw new Error(`missing #${id}`);
  return el as T;
};
const root = byId<HTMLDivElement>('scroll');
const spacer = byId<HTMLDivElement>('spacer');
const sticky = byId<HTMLDivElement>('sticky');
const empty = byId<HTMLDivElement>('empty');

const colorClass = new Map<string, string>();
const colorSheet = document.head.appendChild(document.createElement('style'));
function cls(light: string, dark: string): string {
  const k = light + dark;
  const known = colorClass.get(k);
  if (known) return known;
  const c = `k${colorClass.size}`;
  colorClass.set(k, c);
  const sheet = colorSheet.sheet;
  sheet?.insertRule(`.${c}{color:${light}}`);
  sheet?.insertRule(`@media (prefers-color-scheme: dark){.${c}{color:${dark}}}`);
  return c;
}

const fileIndex = (path: string): number => S.files.findIndex((x) => x.path === path);
const threadsOf = (path: string): ReviewThread[] => S.byFile.get(path) ?? [];
const fileAt = (f: number): PageFile => {
  const file = S.files[f];
  if (!file) throw new Error(`no file ${f}`);
  return file;
};
const rowsOf = (f: number): Row[] => buildFileRows(fileAt(f), f, threadsOf(fileAt(f).path));
const bump = (file: PageFile): void => {
  file.version = (file.version ?? 0) + 1;
};

function rebuild(): void {
  S.rows = S.fileRows.flat();
  if (!S.done) S.rows.push({ k: 'load', f: -1, key: 'load' });
  v.setOptions({ ...v.options, count: S.rows.length, estimateSize: (i) => SIZE[S.rows[i]?.k ?? 'ln'] });
  v._willUpdate();
  render();
}
function rebuildFile(f: number): void {
  const rows = rowsOf(f);
  S.fileRows[f] = rows;
  for (const r of rows) invalidate(r.key);
  rebuild();
}

const minibar = (a: number, d: number): string => {
  const na = a + d ? Math.round((a / (a + d)) * 5) : 0;
  const cells = [0, 1, 2, 3, 4].map((i) => {
    const color = i < na ? 'bg-add-rib' : d ? 'bg-del-rib' : 'bg-press';
    return `<i class="size-1.5 rounded-[1.5px] ${color}"></i>`;
  });
  return `<span class="inline-flex gap-px">${cells.join('')}</span>`;
};
const ICON_BTN = 'inline-flex size-[26px] flex-none items-center justify-center rounded-md text-fg2 hover:bg-hover';
function fileHeaderHtml(f: number, pinned: boolean): string {
  const file = fileAt(f);
  const dir = file.path.replace(/[^/]*$/, '');
  const count = threadsOf(file.path).length;
  const renamed = file.oldPath ? `<span class="text-muted">${esc(file.oldPath)} →</span>` : '';
  const frame = pinned
    ? 'glass border-b border-glassb border-t-0 shadow-[inset_0_1px_0_var(--glass-hi)]'
    : 'border-y border-line bg-ed2';
  return `<div class="flex h-[38px] items-center gap-[7px] pr-2 pl-2.5 whitespace-nowrap ${frame}" data-f="${f}">
    <button class="flex text-muted" data-act="collapse" aria-label="${file.collapsed ? 'Expand' : 'Collapse'} ${esc(file.path)}">${I(file.collapsed ? 'chevR' : 'chevD', 13)}</button>${fileIcon(file.path)}
    <span class="min-w-0 overflow-hidden text-[12.5px] text-ellipsis" role="heading" aria-level="3" aria-label="${esc(file.path)}">${renamed}<span class="text-muted">${esc(dir)}</span><b class="font-semibold">${esc(file.path.slice(dir.length))}</b></span>
    <span class="inline-flex h-4 min-w-4 items-center justify-center rounded bg-cap px-[3px] font-mono text-[10.5px] font-semibold text-muted">${file.status}</span>
    <span class="font-mono text-[11.5px]"><span class="text-ok">+${file.additions}</span> <span class="text-err">−${file.deletions}</span></span>${minibar(file.additions, file.deletions)}
    ${count ? `<span class="ml-1 inline-flex items-center gap-0.5 text-[11px] text-muted">${I('bubble', 13)}${count}</span>` : ''}
    <span class="ml-auto flex flex-none items-center gap-1"><span class="inline-flex flex-none gap-0.5 rounded-md border border-capb bg-cap p-0.5 [&>span]:inline-flex [&>span]:h-5 [&>span]:items-center [&>span]:rounded [&>span]:px-[7px] [&>span]:text-xs [&>span]:text-fg2"><span class="bg-raised !text-fg shadow-[0_1px_2px_rgba(0,0,0,.15),0_0_0_.5px_var(--line2)] dark:bg-[#3c3c3c]">Diff</span><span>Edit</span></span>
      <button class="${ICON_BTN}" title="Comment on file" aria-label="Comment on file" data-act="filecomment">${I('bubblePlus', 15)}</button><span class="${ICON_BTN}" title="Open in editor">${I('openExt', 15)}</span><span class="${ICON_BTN}">${I('more', 15)}</span>
      <button class="inline-flex h-[26px] items-center gap-1.5 rounded-md px-2 text-xs text-fg2" data-act="viewed"><span class="inline-flex size-3.5 flex-none items-center justify-center rounded border ${file.viewed ? 'border-accent bg-accent text-white' : 'border-line2 bg-raised'}">${file.viewed ? I('check', 10) : ''}</span>Viewed</button>
    </span></div>`;
}

const LINE_STYLE = {
  c: { row: '', no: '', rib: '', word: '' },
  a: { row: 'bg-add-bg', no: 'bg-add-num text-muted', rib: 'bg-add-rib', word: '[&_.w]:bg-add-w' },
  d: { row: 'bg-del-bg', no: 'bg-del-num text-muted', rib: 'bg-del-rib', word: '[&_.w]:bg-del-w' },
} as const;
const NO = 'pr-1.5 text-right text-[11px] text-faint';

function rowHtml(r: Row): string {
  switch (r.k) {
    case 'fh':
      return fileHeaderHtml(r.f, false);
    case 'hk':
      return `<div class="flex h-7 items-center gap-2 overflow-hidden border-y border-line bg-hunk pr-2 pl-3 font-mono text-[11.5px]/7 whitespace-nowrap text-muted">${esc(r.text)}</div>`;
    case 'gap':
      return '<div class="h-3"></div>';
    case 'load':
      return `<div class="flex h-[60px] items-center justify-center gap-2 text-xs text-muted"><span class="${dots}"><i></i><i></i><i></i></span>Loading more files…</div>`;
    case 'big':
      return `<div class="flex h-24 flex-col items-center justify-center gap-2 border-b border-line text-[12.5px] text-muted"><span>${esc(r.reason)}</span>${r.canLoad ? `<button class="${btn}" data-act="loadbig" data-f="${r.f}">Load diff</button>` : ''}</div>`;
    case 'ln': {
      const tk = S.tokens.get(r.f);
      const toks = tk ? (r.t === 'd' ? tk.old[r.oi] : tk.new[r.ni]) : null;
      const ranges = r.pair ? wordRanges(r) : null;
      const s = LINE_STYLE[r.t];
      return `<div class="group row-hover relative grid h-5 grid-cols-[40px_40px_3px_1fr] font-mono text-xs/5 ${s.row} ${s.word}"><span class="${NO} ${s.no}">${r.o ?? ''}</span><span class="${NO} ${s.no}">${r.n ?? ''}</span><span class="${s.rib}"></span><span class="overflow-hidden pl-3 whitespace-pre text-code">${codeHtml(r.text, toks, ranges, cls)}</span><button class="absolute top-px left-[62px] z-[2] hidden size-[18px] items-center justify-center rounded bg-accent text-white group-hover:flex" aria-label="Add comment" data-act="linecomment">${I('plus', 12)}</button></div>`;
    }
    default:
      return '';
  }
}

const v = new Virtualizer<HTMLDivElement, HTMLDivElement>({
  count: 0,
  getScrollElement: () => root,
  estimateSize: () => 20,
  getItemKey: (i) => S.rows[i]?.key ?? i,
  overscan: 12,
  scrollToFn: elementScroll,
  observeElementRect,
  observeElementOffset,
  measureElement,
  paddingStart: 0,
  onChange: () => render(),
});
v._didMount();

function invalidate(key: string): void {
  const m = S.nodes.get(key);
  if (!m) return;
  if (m.inner && (m.row.k === 'th' || m.row.k === 'fc')) {
    unmountThread(m.inner);
    S.threadEls.delete(m.row.thread.id);
  }
  m.el.remove();
  S.nodes.delete(key);
}

function makeNode(r: Row, index: number): Mounted {
  const el = document.createElement('div');
  el.dataset.index = String(index);
  el.dataset.key = r.key;
  const m: Mounted = { el, row: r };
  if (r.k === 'th' || r.k === 'fc') {
    el.className = 'absolute inset-x-0 top-0 [contain:layout_style]';
    const inner = document.createElement('div');
    inner.className = r.k === 'fc' ? 'border-b border-line px-4 py-2.5' : 'pt-2 pr-4 pb-2.5 pl-[86px]';
    el.appendChild(inner);
    m.inner = inner;
    S.threadEls.set(r.thread.id, inner);
    mountThread(inner, r.thread, api);
  } else {
    el.className = 'absolute inset-x-0 top-0 [contain:strict]';
    el.style.height = `${SIZE[r.k]}px`;
    el.innerHTML = rowHtml(r);
  }
  return m;
}

let stickyFile = -1;
let stickyVersion: number | undefined;
function render(): void {
  const items = v.getVirtualItems();
  const seen = new Set<string>();
  for (const it of items) {
    const r = S.rows[it.index];
    if (!r) continue;
    let m = S.nodes.get(r.key);
    if (m && m.row !== r) {
      invalidate(r.key);
      m = undefined;
    }
    if (!m) {
      m = makeNode(r, it.index);
      spacer.appendChild(m.el);
      S.nodes.set(r.key, m);
      if (MEASURED.has(r.k)) v.measureElement(m.el);
    }
    if (m.y !== it.start) {
      m.el.style.transform = `translateY(${it.start}px)`;
      m.y = it.start;
    }
    if (m.index !== it.index) {
      m.el.dataset.index = String(it.index);
      m.index = it.index;
    }
    seen.add(r.key);
  }
  for (const k of S.nodes.keys()) if (!seen.has(k)) invalidate(k);
  spacer.style.height = `${v.getTotalSize()}px`;
  updateSticky(items);
  const lastItem = items[items.length - 1];
  if (lastItem) {
    if (S.files.length && !S.done && !S.pending && lastItem.index > S.rows.length - 400) {
      S.pending = true;
      send({ type: 'needMore' });
    }
    queueHighlight(items);
  }
}

function updateSticky(items: VirtualItem[]): void {
  const top = (v.scrollOffset ?? 0) + S.inset;
  let cur: VirtualItem | null = null;
  let nextHeader: VirtualItem | null = null;
  for (const it of items) {
    if (it.end > top && !cur) cur = it;
    else if (cur && S.rows[it.index]?.k === 'fh') {
      nextHeader = it;
      break;
    }
  }
  const r = cur && S.rows[cur.index];
  if (cur && r && r.f >= 0 && r.f !== S.topFile) {
    S.topFile = r.f;
    send({ type: 'visibleFile', path: fileAt(r.f).path });
  }
  if (!cur || !r || r.f < 0 || (r.k === 'fh' && cur.start >= top - 1)) {
    sticky.style.visibility = 'hidden';
    return;
  }
  sticky.style.visibility = 'visible';
  const file = fileAt(r.f);
  if (r.f !== stickyFile || stickyVersion !== file.version) {
    stickyFile = r.f;
    stickyVersion = file.version;
    sticky.innerHTML = fileHeaderHtml(r.f, true);
  }
  const push = nextHeader ? Math.min(0, nextHeader.start - top - SIZE.fh) : 0;
  sticky.style.transform = `translateY(${S.inset + push}px)`;
}

const worker = new Worker('diffpane-worker.js', { type: 'module' });
const hlQueue: number[] = [];
let hlBusy = 0;
worker.onmessage = ({ data }: MessageEvent<HighlightResult>) => {
  hlBusy--;
  if (!data.none) {
    S.tokens.set(data.f, data);
    for (const r of S.fileRows[data.f] ?? []) if (r.k === 'ln') invalidate(r.key);
    render();
  }
  pumpHighlight();
};
function queueHighlight(items: VirtualItem[]): void {
  const files = new Set<number>();
  for (const it of items) {
    const f = S.rows[it.index]?.f;
    if (f !== undefined && f >= 0) files.add(f);
  }
  const lastItem = items[items.length - 1];
  const last = lastItem ? (S.rows[lastItem.index]?.f ?? -1) : -1;
  if (last >= 0) for (let f = last + 1; f < Math.min(last + 3, S.files.length); f++) files.add(f);
  for (const f of files) {
    if (!S.hlQueued.has(f)) {
      S.hlQueued.add(f);
      hlQueue.unshift(f);
    }
  }
  pumpHighlight();
}
function pumpHighlight(): void {
  while (hlBusy < 2) {
    const f = hlQueue.shift();
    if (f === undefined) return;
    const file = fileAt(f);
    const oldL: string[] = [];
    const newL: string[] = [];
    for (const hk of file.hunks) {
      for (const [t, , , text] of hk.lines) {
        if (t !== 'a') oldL.push(text);
        if (t !== 'd') newL.push(text);
      }
    }
    hlBusy++;
    const request: HighlightRequest = { f, path: file.path, oldText: oldL.join('\n'), newText: newL.join('\n') };
    worker.postMessage(request);
  }
}

const api: ThreadApi = {
  post: (t, text) => send({ type: 'post', thread: t.id, text }),
  ask: (t) => send({ type: 'askClaude', thread: t.id }),
  cancel: (t) => send({ type: 'remove', thread: t.id }),
  resolve: (t) => send({ type: 'remove', thread: t.id }),
};

function onAct(e: Event): void {
  const target = e.target as Element;
  const b = target.closest<HTMLElement>('[data-act]');
  if (!b) return;
  const key = b.closest<HTMLElement>('[data-key]')?.dataset.key;
  const node = key ? S.nodes.get(key) : undefined;
  const r = node?.row;
  const f = r ? r.f : Number(b.closest<HTMLElement>('[data-f]')?.dataset.f);
  const file = S.files[f];
  if (!file) return;
  switch (b.dataset.act) {
    case 'collapse':
      file.collapsed = !file.collapsed;
      bump(file);
      rebuildFile(f);
      if (!node) scrollToRow(`fh${f}`);
      break;
    case 'viewed':
      file.viewed = !file.viewed;
      file.collapsed = file.viewed;
      bump(file);
      rebuildFile(f);
      break;
    case 'loadbig':
      file.expanded = true;
      rebuildFile(f);
      break;
    case 'filecomment':
      send({ type: 'createThread', path: file.path });
      break;
    case 'linecomment':
      if (r?.k === 'ln') {
        send({ type: 'createThread', path: file.path, side: r.t === 'd' ? 'old' : 'new', line: (r.t === 'd' ? r.o : r.n) ?? undefined });
      }
      break;
  }
}
spacer.addEventListener('click', onAct);
sticky.addEventListener('click', onAct);

function scrollToRow(key: string): void {
  const index = S.rows.findIndex((x) => x.key === key);
  const item = v.measurementsCache[index];
  if (item) root.scrollTop = item.start - S.inset;
}

let firstPainted = false;
function load(batch: FileDiff[], done: boolean): void {
  for (const file of batch) {
    const f = S.files.push(file) - 1;
    S.fileRows[f] = rowsOf(f);
  }
  S.done = done;
  S.pending = false;
  empty.hidden = S.files.length > 0;
  rebuild();
  if (!firstPainted && S.files.length) {
    firstPainted = true;
    requestAnimationFrame(() => requestAnimationFrame(() => send({ type: 'firstPaint' })));
  }
}

function thread(t: ReviewThread): void {
  const known = S.threads.has(t.id);
  S.threads.set(t.id, t);
  const path = t.anchor.path;
  S.byFile.set(path, [...threadsOf(path).filter((x) => x.id !== t.id), t]);
  const f = fileIndex(path);
  if (f < 0) return;
  if (known) {
    const el = S.threadEls.get(t.id);
    if (el) mountThread(el, t, api);
  } else {
    bump(fileAt(f));
    rebuildFile(f);
  }
}

function removeThread(id: number): void {
  const t = S.threads.get(id);
  if (!t) return;
  S.threads.delete(id);
  const path = t.anchor.path;
  S.byFile.set(path, threadsOf(path).filter((x) => x.id !== id));
  const f = fileIndex(path);
  if (f < 0) return;
  bump(fileAt(f));
  rebuildFile(f);
}

function reveal(path: string): void {
  const f = fileIndex(path);
  if (f >= 0) scrollToRow(`fh${f}`);
}

function setInset(px: number): void {
  S.inset = px;
  v.setOptions({ ...v.options, paddingStart: px });
  rebuild();
}

const BENCH_SECONDS = 3;
const BENCH_PX_PER_SECOND = 6000;
function benchScroll(): void {
  const dts: number[] = [];
  root.scrollTop = 0;
  let last = 0;
  let start = 0;
  const step = (t: number): void => {
    if (start) {
      dts.push(t - last);
      root.scrollTop += BENCH_PX_PER_SECOND * ((t - last) / 1000);
    } else start = t;
    last = t;
    if (t - start < BENCH_SECONDS * 1000) {
      requestAnimationFrame(step);
      return;
    }
    dts.sort((a, b) => a - b);
    const at = (q: number): number => dts[Math.min(dts.length - 1, Math.floor(dts.length * q))] ?? 0;
    send({ type: 'scrollStats', frames: dts.length, p50: at(0.5), p95: at(0.95), max: at(1) });
  };
  requestAnimationFrame(step);
}

const dp: HostApi = { load, thread, removeThread, reveal, setInset, benchScroll };
window.dp = dp;
send({ type: 'ready' });
