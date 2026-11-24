import { build } from 'esbuild';
import { execFileSync } from 'node:child_process';
import { copyFileSync, mkdirSync, readdirSync, rmSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const here = dirname(fileURLToPath(import.meta.url));
const out = join(here, '..', 'diffpilot', 'Features', 'DiffPane', 'Web');

mkdirSync(out, { recursive: true });
for (const name of readdirSync(out)) rmSync(join(out, name));

await build({
  entryPoints: { diffpane: join(here, 'src/main.ts'), 'diffpane-worker': join(here, 'src/worker.ts') },
  outdir: out,
  chunkNames: 'dp-[name]-[hash]',
  bundle: true,
  splitting: true,
  minify: true,
  format: 'esm',
  target: 'safari18',
  logLevel: 'warning',
});
execFileSync(
  join(here, 'node_modules/.bin/tailwindcss'),
  ['--input', join(here, 'src/styles.css'), '--output', join(out, 'diffpane.css'), '--minify'],
  { stdio: ['ignore', 'ignore', 'inherit'] },
);
copyFileSync(join(here, 'src/index.html'), join(out, 'diffpane.html'));
