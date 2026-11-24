const P = (d, extra = '') => `<path d="${d}" ${extra}/>`;
const FILL = 'fill="currentColor" stroke="none"';
const ICONS = {
  chevR: P('M9.5 6l6 6-6 6'),
  chevD: P('M6 9.5l6 6 6-6'),
  bubble: P('M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8.5a1.5 1.5 0 0 1-1.5 1.5h-8l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5V7A1.5 1.5 0 0 1 5 5.5z'),
  bubblePlus: P('M5 5.5h14a1.5 1.5 0 0 1 1.5 1.5v8.5a1.5 1.5 0 0 1-1.5 1.5h-8l-4.5 3.5V17H5a1.5 1.5 0 0 1-1.5-1.5V7A1.5 1.5 0 0 1 5 5.5zM12 8.5v5M9.5 11h5'),
  openExt: P('M13.5 4.5h6v6M19.5 4.5l-8 8M17.5 13.5v5a1.5 1.5 0 0 1-1.5 1.5H6a1.5 1.5 0 0 1-1.5-1.5V8A1.5 1.5 0 0 1 6 6.5h5'),
  more: '<circle cx="6" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="12" cy="12" r="1.5" fill="currentColor" stroke="none"/><circle cx="18" cy="12" r="1.5" fill="currentColor" stroke="none"/>',
  check: P('M5 12.5l4.5 4.5L19 7.5'),
  plus: P('M12 5v14M5 12h14'),
  expand: P('M12 4v6M9 7l3-3 3 3M12 20v-6M9 17l3 3 3-3'),
  sparkle: P('M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z', 'stroke-linejoin="round"') + P('M18.5 15.5l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7z'),
  sparkleF: P('M12 3.5l1.9 5.1 5.1 1.9-5.1 1.9L12 17.5l-1.9-5.1L5 10.5l5.1-1.9z', FILL) + P('M18.5 15.5l.7 1.8 1.8.7-1.8.7-.7 1.8-.7-1.8-1.8-.7 1.8-.7z', FILL),
};
export const I = (n, s = 16) =>
  `<svg class="ic" width="${s}" height="${s}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round">${ICONS[n]}</svg>`;

const FT = { ts: ['TS', '#3178c6'], tsx: ['TSX', '#3178c6'], md: ['MD', '#71717a'], mdx: ['MDX', '#71717a'], json: ['{ }', '#c99a2e'], yml: ['YML', '#cb3c6b'], yaml: ['YML', '#cb3c6b'], swift: ['SW', '#f05138'], kt: ['KT', '#7f52ff'], mm: ['MM', '#438eff'], m: ['M', '#438eff'], h: ['H', '#438eff'] };
export function fileIcon(name, s = 16) {
  const [lbl, col] = FT[name.split('.').pop()] || ['', '#71717a'];
  return `<svg class="ic" width="${s}" height="${s}" viewBox="0 0 16 16"><path d="M3.5 1.5h6l3 3v10h-9z" fill="${col}" fill-opacity=".16" stroke="${col}" stroke-width="1"/><text x="8" y="12" text-anchor="middle" font-size="${lbl.length > 2 ? 4.2 : 5}" font-weight="700" fill="${col}" font-family="-apple-system,sans-serif">${lbl}</text></svg>`;
}
