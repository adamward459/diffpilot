import { createHighlighterCore } from 'shiki/core';
import type { HighlighterCore } from 'shiki/core';
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript';
import type { HighlightRequest, HighlightResult, Token } from './bridge.ts';

const LANGS: Record<string, string> = {
  ts: 'typescript', mts: 'typescript', cts: 'typescript', tsx: 'tsx', js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'jsx',
  json: 'json', md: 'markdown', mdx: 'markdown', yml: 'yaml', yaml: 'yaml', swift: 'swift', kt: 'kotlin', kts: 'kotlin',
  java: 'java', m: 'objective-c', mm: 'objective-cpp', h: 'objective-c', c: 'c', cpp: 'cpp', css: 'css', sh: 'shellscript', rb: 'ruby',
};
type Loader = Parameters<HighlighterCore['loadLanguage']>[0];
const LOADERS: Record<string, () => Loader> = {
  typescript: () => import('@shikijs/langs/typescript'), tsx: () => import('@shikijs/langs/tsx'), javascript: () => import('@shikijs/langs/javascript'),
  jsx: () => import('@shikijs/langs/jsx'), json: () => import('@shikijs/langs/json'), markdown: () => import('@shikijs/langs/markdown'),
  yaml: () => import('@shikijs/langs/yaml'), swift: () => import('@shikijs/langs/swift'), kotlin: () => import('@shikijs/langs/kotlin'),
  java: () => import('@shikijs/langs/java'), 'objective-c': () => import('@shikijs/langs/objective-c'), 'objective-cpp': () => import('@shikijs/langs/objective-cpp'),
  c: () => import('@shikijs/langs/c'), cpp: () => import('@shikijs/langs/cpp'), css: () => import('@shikijs/langs/css'),
  shellscript: () => import('@shikijs/langs/shellscript'), ruby: () => import('@shikijs/langs/ruby'),
};

const highlighter = createHighlighterCore({
  themes: [import('@shikijs/themes/dark-plus'), import('@shikijs/themes/light-plus')],
  langs: [],
  engine: createJavaScriptRegexEngine({ forgiving: true }),
});
const loaded = new Map<string, Promise<void>>();

function tokenize(hl: HighlighterCore, lang: string, text: string): Token[][] {
  const { tokens } = hl.codeToTokens(text, { lang, themes: { light: 'light-plus', dark: 'dark-plus' }, defaultColor: false });
  return tokens.map((line) => line.map((t) => [t.content.length, t.htmlStyle?.['--shiki-light'] ?? '', t.htmlStyle?.['--shiki-dark'] ?? ''] as Token));
}

const ctx = self as unknown as {
  onmessage: (e: MessageEvent<HighlightRequest>) => void;
  postMessage(result: HighlightResult): void;
};

ctx.onmessage = async ({ data: { f, path, oldText, newText } }) => {
  const lang = LANGS[path.split('.').pop()?.toLowerCase() ?? ''];
  const load = lang ? LOADERS[lang] : undefined;
  if (!lang || !load) return ctx.postMessage({ f, none: true });
  const hl = await highlighter;
  let ready = loaded.get(lang);
  if (!ready) {
    ready = hl.loadLanguage(load());
    loaded.set(lang, ready);
  }
  await ready;
  ctx.postMessage({ f, old: tokenize(hl, lang, oldText), new: tokenize(hl, lang, newText) });
};
