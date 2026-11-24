import { h, render } from 'preact';
import { useRef, useState, useEffect } from 'preact/hooks';
import { I } from './icons.js';
import { esc } from './rows.js';
const fmt = (s) =>
  esc(s)
    .replace(/`([^`\n]+)`/g, '<code class="i">$1</code>')
    .replace(/@claude\b/gi, '<span class="mention cl">@claude</span>');
const raw = (html) => h('span', { dangerouslySetInnerHTML: { __html: html } });

const PEOPLE = [{ id: 'claude', name: 'Claude', sub: 'Ask Claude in this thread' }];

function Avatar({ who }) {
  return who === 'Claude'
    ? h('span', { class: 'av cl' }, raw(I('sparkleF', 13)))
    : h('span', { class: 'av' }, who.split(' ').map((w) => w[0]).join(''));
}

function Comment({ c }) {
  const body = c.isStreaming && !c.body
    ? h('span', { class: 'thinking' }, h('i'), h('i'), h('i'))
    : [raw(fmt(c.body)), c.isStreaming ? h('span', { class: 'caret' }) : null];
  return h('div', { class: 'cm' },
    h(Avatar, { who: c.author }),
    h('div', { style: 'flex:1;min-width:0' },
      h('div', { class: 'who' }, h('b', null, c.author)),
      h('div', { class: 'bd' }, body)));
}

function Composer({ t, api, autoFocus }) {
  const ref = useRef();
  const [text, setText] = useState('');
  const [pop, setPop] = useState(null);
  useEffect(() => { if (autoFocus) ref.current?.focus({ preventScroll: true }); }, []);

  const query = (value, caret) => {
    const m = /(^|\s)@(\w*)$/.exec(value.slice(0, caret));
    if (!m) return setPop(null);
    const list = PEOPLE.filter((p) => p.name.toLowerCase().startsWith(m[2].toLowerCase()) || p.id.startsWith(m[2].toLowerCase()));
    setPop(list.length ? { list, at: caret - m[2].length - 1, hl: 0 } : null);
  };
  const pick = (p) => {
    const el = ref.current;
    const tag = p.id === 'claude' ? '@claude ' : `@${p.name.split(' ')[0].toLowerCase()} `;
    const next = text.slice(0, pop.at) + tag + text.slice(el.selectionStart);
    setText(next);
    setPop(null);
    requestAnimationFrame(() => { el.selectionStart = el.selectionEnd = pop.at + tag.length; el.focus(); });
  };
  const submit = () => {
    if (!text.trim()) return;
    api.post(t, text.trim());
    setText('');
  };
  const onKey = (e) => {
    e.stopPropagation();
    if (pop) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        setPop({ ...pop, hl: (pop.hl + (e.key === 'ArrowDown' ? 1 : -1) + pop.list.length) % pop.list.length });
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') { e.preventDefault(); return pick(pop.list[pop.hl]); }
      if (e.key === 'Escape') return setPop(null);
    }
    if (e.key === 'Enter' && e.metaKey) { e.preventDefault(); submit(); }
    if (e.key === 'Escape' && t.comments.length === 0) api.cancel(t);
  };

  return h('div', { class: 'reply' },
    pop && h('div', { class: 'mpop' }, pop.list.map((p, i) =>
      h('div', { class: 'mi' + (i === pop.hl ? ' hl' : ''), onMouseDown: (e) => { e.preventDefault(); pick(p); } },
        p.id === 'claude' ? raw(I('sparkleF', 14)) : h(Avatar, { who: p.name }), p.name, h('span', { class: 'sub' }, p.sub)))),
    h('textarea', {
      ref, class: 'ta', rows: 1, value: text,
      placeholder: t.comments.length ? 'Reply… type @claude to ask Claude' : 'Leave a comment… type @claude to ask Claude',
      onInput: (e) => { setText(e.currentTarget.value); query(e.currentTarget.value, e.currentTarget.selectionStart); },
      onKeyDown: onKey,
    }),
    (text || t.comments.length === 0) && h('div', { class: 'comp-ft' },
      /@claude\b/i.test(text) ? h('span', { class: 'chip cl' }, raw(I('sparkle', 11)), 'Claude will reply') : null,
      h('span', { style: 'flex:1' }),
      t.comments.length === 0 ? h('button', { class: 'btn', onClick: () => api.cancel(t) }, 'Cancel') : null,
      h('button', { class: 'btn pri', onClick: submit }, t.comments.length ? 'Reply' : 'Comment', h('span', { class: 'kbd' }, '⌘↩'))));
}

function Thread({ t, api }) {
  const { anchor } = t;
  const label = anchor.kind === 'file' ? 'File comment' : `Line ${anchor.line}${anchor.side === 'old' ? ' (old)' : ''}`;
  const claude = t.comments.some((c) => c.author === 'Claude');
  const busy = t.comments.some((c) => c.isStreaming);
  return h('div', { class: 'th' + (claude ? ' claude' : '') },
    h('div', { class: 'th-h' },
      h('span', null, label),
      t.comments.length === 0 ? h('span', { class: 'chip amber' }, 'Draft') : null,
      h('span', { class: 'acts' },
        h('button', { class: 'btn cl', disabled: busy, onClick: () => api.ask(t) }, raw(I('sparkle', 12)), 'Ask Claude'),
        t.comments.length ? h('button', { class: 'btn', onClick: () => api.resolve(t) }, 'Resolve') : null)),
    t.comments.map((c) => h(Comment, { key: c.id, c })),
    h(Composer, { t, api, autoFocus: t.comments.length === 0 }));
}

export const mountThread = (el, t, api) => render(h(Thread, { t, api }), el);
export const unmountThread = (el) => render(null, el);
