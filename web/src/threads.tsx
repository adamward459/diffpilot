import { render } from 'preact';
import type { ComponentChildren } from 'preact';
import { useEffect, useRef, useState } from 'preact/hooks';
import type { ReviewThread, Severity, ThreadComment } from './bridge.ts';
import { I } from './icons.ts';
import type { IconName } from './icons.ts';
import { esc } from './rows.ts';
import { btn, btnClaude, btnPrimary, chip, dots } from './ui.ts';

export interface ThreadApi {
  post(thread: ReviewThread, text: string): void;
  ask(thread: ReviewThread): void;
  cancel(thread: ReviewThread): void;
  resolve(thread: ReviewThread): void;
}

interface Person {
  id: string;
  name: string;
  sub: string;
}
interface Popup {
  list: Person[];
  at: number;
  hl: number;
}

const fmt = (s: string): string =>
  esc(s)
    .replace(/`([^`\n]+)`/g, '<code class="rounded bg-cap px-1 py-px font-mono text-[11.5px]">$1</code>')
    .replace(/@claude\b/gi, '<span class="font-semibold text-claude">@claude</span>');

const PEOPLE: Person[] = [{ id: 'claude', name: 'Claude', sub: 'Ask Claude in this thread' }];

function Raw({ html }: { html: string }) {
  return <span dangerouslySetInnerHTML={{ __html: html }} />;
}
const Icon = ({ name, size }: { name: IconName; size: number }) => <Raw html={I(name, size)} />;

function Avatar({ who }: { who: string }) {
  const base = 'flex size-[22px] flex-none items-center justify-center rounded-full text-[10px] font-bold text-white';
  return who === 'Claude' ? (
    <span class={`${base} bg-claude`}>
      <Icon name="sparkle" size={13} />
    </span>
  ) : (
    <span class={`${base} bg-[#616161]`}>
      {who
        .split(' ')
        .map((w) => w[0])
        .join('')}
    </span>
  );
}

function Comment({ c, first }: { c: ThreadComment; first: boolean }) {
  const body: ComponentChildren =
    c.isStreaming && !c.body ? (
      <span class={dots}>
        <i />
        <i />
        <i />
      </span>
    ) : (
      <>
        <Raw html={fmt(c.body)} />
        {c.isStreaming ? <span class="ml-px inline-block h-3.5 w-[7px] animate-caret bg-claude align-[-2px]" /> : null}
      </>
    );
  return (
    <div class={`flex gap-2.5 px-3 pt-2.5 pb-2 ${first ? '' : 'border-t border-line'}`}>
      <Avatar who={c.author} />
      <div class="min-w-0 flex-1">
        <div class="flex items-center gap-1.5 text-xs whitespace-nowrap">
          <b class="font-semibold">{c.author}</b>
        </div>
        <div class="mt-0.5 text-[13px]/[19px] break-words whitespace-pre-wrap text-fg [overflow-wrap:anywhere]">{body}</div>
      </div>
    </div>
  );
}

function Composer({ t, api, autoFocus }: { t: ReviewThread; api: ThreadApi; autoFocus: boolean }) {
  const ref = useRef<HTMLTextAreaElement>(null);
  const [text, setText] = useState('');
  const [pop, setPop] = useState<Popup | null>(null);
  useEffect(() => {
    if (autoFocus) ref.current?.focus({ preventScroll: true });
  }, []);

  const query = (value: string, caret: number) => {
    const m = /(^|\s)@(\w*)$/.exec(value.slice(0, caret));
    const typed = m?.[2];
    if (typed === undefined) return setPop(null);
    const q = typed.toLowerCase();
    const list = PEOPLE.filter((p) => p.name.toLowerCase().startsWith(q) || p.id.startsWith(q));
    setPop(list.length ? { list, at: caret - typed.length - 1, hl: 0 } : null);
  };
  const pick = (p: Person) => {
    const el = ref.current;
    if (!el || !pop) return;
    const tag = `@${p.id} `;
    setText(text.slice(0, pop.at) + tag + text.slice(el.selectionStart));
    setPop(null);
    const at = pop.at + tag.length;
    requestAnimationFrame(() => {
      el.selectionStart = el.selectionEnd = at;
      el.focus();
    });
  };
  const submit = () => {
    if (!text.trim()) return;
    api.post(t, text.trim());
    setText('');
  };
  const onKey = (e: KeyboardEvent) => {
    e.stopPropagation();
    if (pop) {
      if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
        e.preventDefault();
        setPop({ ...pop, hl: (pop.hl + (e.key === 'ArrowDown' ? 1 : -1) + pop.list.length) % pop.list.length });
        return;
      }
      if (e.key === 'Enter' || e.key === 'Tab') {
        e.preventDefault();
        const chosen = pop.list[pop.hl];
        if (chosen) pick(chosen);
        return;
      }
      if (e.key === 'Escape') return setPop(null);
    }
    if (e.key === 'Enter' && e.metaKey) {
      e.preventDefault();
      submit();
    }
    if (e.key === 'Escape' && t.comments.length === 0) api.cancel(t);
  };

  return (
    <div class="relative border-t border-line px-3 pt-2 pb-2.5">
      {pop && (
        <div class="glass absolute bottom-[calc(100%-4px)] left-3 z-[5] min-w-[220px] rounded-xl p-[5px] text-[13px] shadow-[inset_0_1px_0_var(--glass-hi),var(--shadow)]">
          {pop.list.map((p, i) => (
            <div
              key={p.id}
              class={`flex h-[26px] items-center gap-[7px] rounded-[5px] pr-2 pl-1.5 ${i === pop.hl ? 'bg-accent text-on-accent' : ''}`}
              onMouseDown={(e) => {
                e.preventDefault();
                pick(p);
              }}
            >
              <Icon name="sparkle" size={14} />
              {p.name}
              <span class="ml-auto text-[11.5px] opacity-80">{p.sub}</span>
            </div>
          ))}
        </div>
      )}
      <textarea
        ref={ref}
        rows={1}
        value={text}
        placeholder={t.comments.length ? 'Reply… type @claude to ask Claude' : 'Leave a comment… type @claude to ask Claude'}
        class="block min-h-7 w-full resize-none rounded-md border border-line2 bg-ed px-[9px] py-[5px] font-sans text-[12.5px]/[18px] text-fg outline-none [field-sizing:content] focus:border-accent focus:shadow-[0_0_0_3px_var(--accent-soft)]"
        onInput={(e) => {
          setText(e.currentTarget.value);
          query(e.currentTarget.value, e.currentTarget.selectionStart);
        }}
        onKeyDown={onKey}
      />
      {(text || t.comments.length === 0) && (
        <div class="mt-2 flex items-center gap-2">
          {/@claude\b/i.test(text) ? (
            <span class={`${chip} bg-claude-bg text-claude`}>
              <Icon name="sparkle" size={11} />
              Claude will reply
            </span>
          ) : null}
          <span class="flex-1" />
          {t.comments.length === 0 ? (
            <button class={btn} onClick={() => api.cancel(t)}>
              Cancel
            </button>
          ) : null}
          <button class={btnPrimary} onClick={submit}>
            {t.comments.length ? 'Reply' : 'Comment'}
            <span class="text-[11px] opacity-75">⌘↩</span>
          </button>
        </div>
      )}
    </div>
  );
}

const SEVERITY: Record<Severity, { label: string; icon: IconName; text: string; box: string }> = {
  'must-fix': { label: 'Must fix', icon: 'mustFix', text: 'text-err', box: 'border-err/45' },
  suggestion: { label: 'Suggestion', icon: 'suggestion', text: 'text-amber', box: 'border-amber/45' },
  nit: { label: 'Nit', icon: 'nit', text: 'text-muted', box: 'border-transparent' },
  question: { label: 'Question', icon: 'question', text: 'text-accent', box: 'border-accent/45' },
};

function threadLabel(t: ReviewThread): string {
  const { anchor } = t;
  if (anchor.kind === 'change') return 'Review summary';
  if (anchor.kind === 'file') return 'File comment';
  return `Line ${anchor.line}${anchor.side === 'old' ? ' (old)' : ''}`;
}

function findingPlace(t: ReviewThread): string | null {
  if (t.anchor.kind === 'line') return `Line ${t.anchor.line}`;
  return t.finding?.location ?? null;
}

function copyText(text: string): void {
  const area = document.createElement('textarea');
  area.value = text;
  area.style.position = 'fixed';
  area.style.opacity = '0';
  document.body.appendChild(area);
  area.select();
  document.execCommand('copy');
  area.remove();
}

function Replies({ t, api, open }: { t: ReviewThread; api: ThreadApi; open: boolean }) {
  return (
    <>
      {t.comments.slice(1).map((c) => (
        <Comment key={c.id} c={c} first={false} />
      ))}
      {open ? <Composer t={t} api={api} autoFocus /> : null}
    </>
  );
}

function FindingBanner({ t, api }: { t: ReviewThread; api: ThreadApi }) {
  const finding = t.finding;
  const [collapsed, setCollapsed] = useState(finding?.severity === 'nit');
  const [replying, setReplying] = useState(false);
  if (!finding) return null;
  const sev = SEVERITY[finding.severity];
  const first = t.comments[0];
  const place = findingPlace(t);
  return (
    <div class={`glass-card overflow-hidden rounded-xl border ${sev.box}`}>
      <div class="flex h-[38px] items-center gap-2 pr-2 pl-3 text-[13px] whitespace-nowrap">
        <span class={`flex ${sev.text}`}>
          <Icon name={sev.icon} size={15} />
        </span>
        <b class="min-w-0 overflow-hidden font-semibold text-ellipsis text-fg">{finding.title}</b>
        <span class={`text-xs font-medium ${sev.text}`}>{sev.label}</span>
        <span class="ml-auto flex items-center gap-1">
          {collapsed ? null : (
            <>
              <button class={btn} onClick={() => setReplying(true)}>
                Reply
              </button>
              <button class={btn} onClick={() => api.resolve(t)}>
                Resolve
              </button>
            </>
          )}
          <button
            class="flex size-[22px] items-center justify-center rounded text-muted hover:bg-hover"
            aria-label={collapsed ? 'Expand finding' : 'Collapse finding'}
            onClick={() => setCollapsed(!collapsed)}
          >
            <Icon name={collapsed ? 'chevD' : 'chevU'} size={11} />
          </button>
        </span>
      </div>
      {collapsed ? null : (
        <>
          <div class="px-3 pl-[38px] text-[13px]/[19px] break-words whitespace-pre-wrap text-fg [overflow-wrap:anywhere]">
            {first ? <Raw html={fmt(first.body)} /> : null}
          </div>
          <div class="flex items-center gap-1.5 px-3 pt-1.5 pb-2.5 pl-[38px] text-[11.5px] text-muted">
            <span class="inline-flex items-center gap-1 text-claude">
              <Icon name="sparkle" size={11} />
              Claude
            </span>
            {place ? (
              <>
                <span>·</span>
                <span>{place}</span>
              </>
            ) : null}
          </div>
          <Replies t={t} api={api} open={replying} />
        </>
      )}
    </div>
  );
}

function SummaryCard({ t, api }: { t: ReviewThread; api: ThreadApi }) {
  const [replying, setReplying] = useState(false);
  const first = t.comments[0];
  return (
    <div class="glass-card overflow-hidden rounded-xl border border-claude-b">
      <div class="flex h-[38px] items-center gap-2 pr-2 pl-3 text-[13px] whitespace-nowrap">
        <span class="flex text-claude">
          <Icon name="sparkle" size={13} />
        </span>
        <b class="font-semibold text-fg">Review summary</b>
        <span class="text-xs text-muted">Whole change · Claude</span>
        <span class="ml-auto flex gap-1">
          <button class={btn} onClick={() => copyText(first?.body ?? '')}>
            <Icon name="copy" size={11} />
            Copy as Markdown
          </button>
          <button class={btn} onClick={() => setReplying(true)}>
            Reply
          </button>
          <button class={btn} onClick={() => api.resolve(t)}>
            Resolve
          </button>
        </span>
      </div>
      <div class="px-3 pb-3 text-[13px]/[19px] break-words whitespace-pre-wrap text-fg [overflow-wrap:anywhere]">
        {first ? <Raw html={fmt(first.body)} /> : null}
      </div>
      <Replies t={t} api={api} open={replying} />
    </div>
  );
}

const isSummary = (t: ReviewThread): boolean =>
  t.anchor.kind === 'change' && !t.finding && t.comments[0]?.author === 'Claude';

function Thread({ t, api }: { t: ReviewThread; api: ThreadApi }) {
  if (t.finding) return <FindingBanner t={t} api={api} />;
  if (isSummary(t)) return <SummaryCard t={t} api={api} />;
  const claude = t.comments.some((c) => c.author === 'Claude');
  const busy = t.comments.some((c) => c.isStreaming);
  return (
    <div class={`glass-card overflow-hidden rounded-xl border ${claude ? 'border-claude-b' : 'border-transparent'}`}>
      <div class="flex h-[34px] items-center gap-2 border-b border-line pr-2 pl-3 text-xs whitespace-nowrap text-fg2">
        <span>{threadLabel(t)}</span>
        {t.comments.length === 0 ? <span class={`${chip} bg-amber-soft text-amber`}>Draft</span> : null}
        <span class="ml-auto flex gap-1">
          <button class={btnClaude} disabled={busy} onClick={() => api.ask(t)}>
            <Icon name="sparkle" size={12} />
            Ask Claude
          </button>
          {t.comments.length ? (
            <button class={btn} onClick={() => api.resolve(t)}>
              Resolve
            </button>
          ) : null}
        </span>
      </div>
      {t.comments.map((c, i) => (
        <Comment key={c.id} c={c} first={i === 0} />
      ))}
      <Composer t={t} api={api} autoFocus={t.comments.length === 0} />
    </div>
  );
}

export const mountThread = (el: Element, t: ReviewThread, api: ThreadApi): void => render(<Thread t={t} api={api} />, el);
export const unmountThread = (el: Element): void => render(null, el);
