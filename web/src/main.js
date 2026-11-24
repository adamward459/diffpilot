import { Virtualizer, elementScroll, observeElementOffset, observeElementRect, measureElement } from '@tanstack/virtual-core';
import { mountThread, unmountThread } from './threads.js';
import { I, fileIcon } from './icons.js';
import { SIZE, buildFileRows, codeHtml, esc, wordRanges } from './rows.js';

const MEASURED = new Set(['th', 'fc']);
const send = (type, fields = {}) => window.webkit.messageHandlers.dp.postMessage({ type, ...fields });

const S = {
  files: [], fileRows: [], rows: [], done: false, pending: false, inset: 0,
  threads: new Map(), byFile: new Map(),
  tokens: new Map(), hlQueued: new Set(),
  nodes: new Map(), threadEls: new Map(),
};

const colorClass = new Map();
const colorSheet = document.head.appendChild(document.createElement('style'));
function cls(light, dark) {
  const k = light + dark;
  let c = colorClass.get(k);
  if (c) return c;
  c = 'k' + colorClass.size;
  colorClass.set(k, c);
  colorSheet.sheet.insertRule(`.${c}{color:${light}}`);
  colorSheet.sheet.insertRule(`@media (prefers-color-scheme: dark){.${c}{color:${dark}}}`);
  return c;
}

const fileIndex = (path) => S.files.findIndex((x) => x.path === path);
const threadsOf = (path) => S.byFile.get(path) || [];
const rowsOf = (f) => buildFileRows(S.files[f], f, threadsOf(S.files[f].path));

function rebuild() {
  S.rows = S.fileRows.flat();
  if (!S.done) S.rows.push({ k: 'load', f: -1, key: 'load' });
  v.setOptions({ ...v.options, count: S.rows.length, estimateSize: (i) => SIZE[S.rows[i].k] });
  v._willUpdate();
  render();
}
function rebuildFile(f) {
  S.fileRows[f] = rowsOf(f);
  for (const r of S.fileRows[f]) invalidate(r.key);
  rebuild();
}

const minibar = (a, d) => {
  const tot = a + d;
  const na = tot ? Math.round((a / tot) * 5) : 0;
  return `<span class="minibar">${[0, 1, 2, 3, 4].map((i) => `<i class="${i < na ? 'a' : d ? 'd' : ''}"></i>`).join('')}</span>`;
};
function fileHeaderHtml(f) {
  const file = S.files[f];
  const dir = file.path.replace(/[^/]*$/, '');
  const count = threadsOf(file.path).length;
  const renamed = file.oldPath ? `<span class="muted">${esc(file.oldPath)} →</span>` : '';
  return `<div class="fh" data-f="${f}">
    <button class="muted" style="display:flex" data-act="collapse" aria-label="${file.collapsed ? 'Expand' : 'Collapse'} ${esc(file.path)}">${I(file.collapsed ? 'chevR' : 'chevD', 13)}</button>${fileIcon(file.path)}
    <span class="path" role="heading" aria-level="3" aria-label="${esc(file.path)}">${renamed}<span class="muted">${esc(dir)}</span><b>${esc(file.path.slice(dir.length))}</b></span>
    <span class="badge">${file.status}</span>
    <span class="mono" style="font-size:11.5px"><span class="add">+${file.additions}</span> <span class="del">−${file.deletions}</span></span>${minibar(file.additions, file.deletions)}
    ${count ? `<span class="cnt" style="margin-left:4px">${I('bubble', 13)}${count}</span>` : ''}
    <span class="acts"><span class="seg"><span class="on">Diff</span><span>Edit</span></span>
      <button class="ib" title="Comment on file" aria-label="Comment on file" data-act="filecomment">${I('bubblePlus', 15)}</button><span class="ib" title="Open in editor">${I('openExt', 15)}</span><span class="ib">${I('more', 15)}</span>
      <button class="viewed" data-act="viewed"><span class="chk ${file.viewed ? 'on' : ''}">${file.viewed ? I('check', 10) : ''}</span>Viewed</button>
    </span></div>`;
}

function rowHtml(r) {
  switch (r.k) {
    case 'fh': return fileHeaderHtml(r.f);
    case 'hk': return `<div class="hk">${esc(r.text)}</div>`;
    case 'gap': return '<div class="gap"></div>';
    case 'load': return '<div class="loading"><span class="thinking"><i></i><i></i><i></i></span>Loading more files…</div>';
    case 'big': return `<div class="big"><span>${esc(r.reason)}</span>${r.canLoad ? `<button class="btn" data-act="loadbig" data-f="${r.f}">Load diff</button>` : ''}</div>`;
    case 'ln': {
      const tk = S.tokens.get(r.f);
      const toks = tk ? (r.t === 'd' ? tk.old[r.oi] : tk.new[r.ni]) : null;
      const ranges = r.pair ? wordRanges(r) : null;
      return `<div class="row ${r.t === 'c' ? '' : r.t}"><span class="no">${r.o ?? ''}</span><span class="no">${r.n ?? ''}</span><span class="rib"></span><span class="code">${codeHtml(r.text, toks, ranges, cls)}</span><button class="addc" aria-label="Add comment" data-act="linecomment">${I('plus', 12)}</button></div>`;
    }
  }
  return '';
}

const root = document.getElementById('scroll');
const spacer = document.getElementById('spacer');
const sticky = document.getElementById('sticky');
const empty = document.getElementById('empty');

const v = new Virtualizer({
  count: 0,
  getScrollElement: () => root,
  estimateSize: () => 20,
  getItemKey: (i) => S.rows[i].key,
  overscan: 12,
  scrollToFn: elementScroll,
  observeElementRect,
  observeElementOffset,
  measureElement,
  paddingStart: 0,
  onChange: () => render(),
});
v._didMount();

function invalidate(key) {
  const el = S.nodes.get(key);
  if (!el) return;
  if (el._thread) {
    unmountThread(el.firstChild);
    S.threadEls.delete(el._thread.id);
  }
  el.remove();
  S.nodes.delete(key);
}

function makeNode(r, index) {
  const el = document.createElement('div');
  el._r = r;
  el.dataset.index = index;
  if (MEASURED.has(r.k)) {
    el.className = 'vr';
    const inner = document.createElement('div');
    inner.className = r.k === 'fc' ? 'blk file' : 'blk';
    el.appendChild(inner);
    el._thread = r.t;
    S.threadEls.set(r.t.id, inner);
    mountThread(inner, r.t, api);
  } else {
    el.className = 'vr fixed';
    el.style.height = SIZE[r.k] + 'px';
    el.innerHTML = rowHtml(r);
  }
  return el;
}

let stickyFile = -1;
function render() {
  const items = v.getVirtualItems();
  const seen = new Set();
  for (const it of items) {
    const r = S.rows[it.index];
    if (!r) continue;
    let el = S.nodes.get(r.key);
    if (el && el._r !== r) {
      invalidate(r.key);
      el = null;
    }
    if (!el) {
      el = makeNode(r, it.index);
      spacer.appendChild(el);
      S.nodes.set(r.key, el);
      if (MEASURED.has(r.k)) v.measureElement(el);
    }
    if (el._y !== it.start) {
      el.style.transform = `translateY(${it.start}px)`;
      el._y = it.start;
    }
    if (el._i !== it.index) {
      el.dataset.index = it.index;
      el._i = it.index;
    }
    seen.add(r.key);
  }
  for (const k of S.nodes.keys()) if (!seen.has(k)) invalidate(k);
  spacer.style.height = v.getTotalSize() + 'px';
  updateSticky(items);
  if (items.length) {
    const last = items[items.length - 1].index;
    if (S.files.length && !S.done && !S.pending && last > S.rows.length - 400) {
      S.pending = true;
      send('needMore');
    }
    queueHighlight(items);
  }
}

function updateSticky(items) {
  const top = v.scrollOffset ?? 0;
  let cur = null;
  let nextHeader = null;
  for (const it of items) {
    if (it.end > top && !cur) cur = it;
    else if (cur && S.rows[it.index]?.k === 'fh') {
      nextHeader = it;
      break;
    }
  }
  const r = cur && S.rows[cur.index];
  if (!r || r.f < 0 || (r.k === 'fh' && cur.start >= top - 1)) {
    sticky.style.visibility = 'hidden';
    return;
  }
  sticky.style.visibility = 'visible';
  if (r.f !== stickyFile || sticky._v !== S.files[r.f]._v) {
    stickyFile = r.f;
    sticky._v = S.files[r.f]._v;
    sticky.innerHTML = fileHeaderHtml(r.f);
  }
  const push = nextHeader ? Math.min(0, nextHeader.start - top - SIZE.fh) : 0;
  sticky.style.transform = `translateY(${S.inset + push}px)`;
}

const worker = new Worker('diffpane-worker.js', { type: 'module' });
const hlQueue = [];
let hlBusy = 0;
worker.onmessage = ({ data }) => {
  hlBusy--;
  if (!data.none) {
    S.tokens.set(data.f, data);
    for (const r of S.fileRows[data.f]) if (r.k === 'ln') invalidate(r.key);
    render();
  }
  pumpHighlight();
};
function queueHighlight(items) {
  const fs = new Set(items.map((it) => S.rows[it.index]?.f).filter((f) => f >= 0));
  const last = S.rows[items[items.length - 1].index]?.f;
  if (last >= 0) for (let f = last + 1; f < Math.min(last + 3, S.files.length); f++) fs.add(f);
  for (const f of fs) {
    if (!S.hlQueued.has(f)) {
      S.hlQueued.add(f);
      hlQueue.unshift(f);
    }
  }
  pumpHighlight();
}
function pumpHighlight() {
  while (hlBusy < 2 && hlQueue.length) {
    const f = hlQueue.shift();
    const file = S.files[f];
    const oldL = [];
    const newL = [];
    for (const hk of file.hunks) {
      for (const [t, , , text] of hk.lines) {
        if (t !== 'a') oldL.push(text);
        if (t !== 'd') newL.push(text);
      }
    }
    hlBusy++;
    worker.postMessage({ f, path: file.path, oldText: oldL.join('\n'), newText: newL.join('\n') });
  }
}

const api = {
  post: (t, text) => send('post', { thread: t.id, text }),
  ask: (t) => send('askClaude', { thread: t.id }),
  cancel: (t) => send('remove', { thread: t.id }),
  resolve: (t) => send('remove', { thread: t.id }),
};

spacer.addEventListener('click', onAct);
sticky.addEventListener('click', onAct);
function onAct(e) {
  const b = e.target.closest('[data-act]');
  if (!b) return;
  const node = b.closest('.vr');
  const r = node?._r;
  const f = r ? r.f : Number(b.closest('[data-f]')?.dataset.f);
  const file = S.files[f];
  switch (b.dataset.act) {
    case 'collapse': {
      file.collapsed = !file.collapsed;
      file._v = (file._v || 0) + 1;
      rebuildFile(f);
      if (!node) scrollToRow(`fh${f}`);
      break;
    }
    case 'viewed':
      file.viewed = !file.viewed;
      file.collapsed = file.viewed;
      file._v = (file._v || 0) + 1;
      rebuildFile(f);
      break;
    case 'loadbig':
      file.expanded = true;
      rebuildFile(f);
      break;
    case 'filecomment':
      send('createThread', { path: file.path });
      break;
    case 'linecomment':
      send('createThread', { path: file.path, side: r.t === 'd' ? 'old' : 'new', line: r.t === 'd' ? r.o : r.n });
      break;
  }
}

function scrollToRow(key) {
  const index = S.rows.findIndex((x) => x.key === key);
  const item = v.measurementsCache[index];
  if (item) root.scrollTop = item.start - S.inset;
}

let firstPainted = false;
function load(batch, done) {
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
    requestAnimationFrame(() => requestAnimationFrame(() => send('firstPaint')));
  }
}

function thread(t) {
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
    S.files[f]._v = (S.files[f]._v || 0) + 1;
    rebuildFile(f);
  }
}

function removeThread(id) {
  const t = S.threads.get(id);
  if (!t) return;
  S.threads.delete(id);
  const path = t.anchor.path;
  S.byFile.set(path, threadsOf(path).filter((x) => x.id !== id));
  const f = fileIndex(path);
  if (f < 0) return;
  S.files[f]._v = (S.files[f]._v || 0) + 1;
  rebuildFile(f);
}

function reveal(path) {
  const f = fileIndex(path);
  if (f >= 0) scrollToRow(`fh${f}`);
}

function setInset(px) {
  S.inset = px;
  v.setOptions({ ...v.options, paddingStart: px });
  rebuild();
}

window.dp = { load, thread, removeThread, reveal, setInset };
send('ready');
