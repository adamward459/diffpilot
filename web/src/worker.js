import { createHighlighterCore } from 'shiki/core';
import { createJavaScriptRegexEngine } from 'shiki/engine/javascript';

const LANGS = {
  ts: 'typescript', mts: 'typescript', cts: 'typescript', tsx: 'tsx', js: 'javascript', mjs: 'javascript', cjs: 'javascript', jsx: 'jsx',
  json: 'json', md: 'markdown', mdx: 'markdown', yml: 'yaml', yaml: 'yaml', swift: 'swift', kt: 'kotlin', kts: 'kotlin',
  java: 'java', m: 'objective-c', mm: 'objective-cpp', h: 'objective-c', c: 'c', cpp: 'cpp', css: 'css', sh: 'shellscript', rb: 'ruby',
};
const LOADERS = {
  typescript: () => import('@shikijs/langs/typescript'), tsx: () => import('@shikijs/langs/tsx'), javascript: () => import('@shikijs/langs/javascript'),
  jsx: () => import('@shikijs/langs/jsx'), json: () => import('@shikijs/langs/json'), markdown: () => import('@shikijs/langs/markdown'),
  yaml: () => import('@shikijs/langs/yaml'), swift: () => import('@shikijs/langs/swift'), kotlin: () => import('@shikijs/langs/kotlin'),
  java: () => import('@shikijs/langs/java'), 'objective-c': () => import('@shikijs/langs/objective-c'), 'objective-cpp': () => import('@shikijs/langs/objective-cpp'),
  c: () => import('@shikijs/langs/c'), cpp: () => import('@shikijs/langs/cpp'), css: () => import('@shikijs/langs/css'),
  shellscript: () => import('@shikijs/langs/shellscript'), ruby: () => import('@shikijs/langs/ruby'),
};

const hlP = createHighlighterCore({
  themes: [import('@shikijs/themes/dark-plus'), import('@shikijs/themes/light-plus')],
  langs: [],
  engine: createJavaScriptRegexEngine({ forgiving: true }),
});
const loaded = new Map();

function tokenize(hl, lang, text) {
  const { tokens } = hl.codeToTokens(text, { lang, themes: { light: 'light-plus', dark: 'dark-plus' }, defaultColor: false });
  return tokens.map((line) => line.map((t) => [t.content.length, t.htmlStyle['--shiki-light'], t.htmlStyle['--shiki-dark']]));
}

self.onmessage = async ({ data: { f, path, oldText, newText } }) => {
  const lang = LANGS[path.split('.').pop().toLowerCase()];
  if (!lang) return self.postMessage({ f, none: true });
  const hl = await hlP;
  if (!loaded.has(lang)) loaded.set(lang, hl.loadLanguage(LOADERS[lang]()));
  await loaded.get(lang);
  const t = performance.now();
  const out = { f, old: tokenize(hl, lang, oldText), new: tokenize(hl, lang, newText) };
  out.ms = performance.now() - t;
  self.postMessage(out);
};
