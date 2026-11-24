export const SYMBOLS = {
  chevR: 'chevron.right',
  chevD: 'chevron.down',
  bubble: 'bubble.left',
  plus: 'plus',
  check: 'checkmark',
  sparkle: 'sparkles',
} as const;
export type IconName = keyof typeof SYMBOLS;

export const symbolUrl = (symbol: string): string => `diffpilot://pane/sf/${symbol}.png`;

const maskStyle = (symbol: string, size: number, extra = ''): string =>
  `width:${size}px;height:${size}px;-webkit-mask:url(${symbolUrl(symbol)}) center/contain no-repeat;mask:url(${symbolUrl(symbol)}) center/contain no-repeat;${extra}`;

export const symbolIcon = (symbol: string, size = 16, extra = ''): string =>
  `<i class="block flex-none bg-current" style="${maskStyle(symbol, size, extra)}"></i>`;

export const I = (name: IconName, size = 16): string => symbolIcon(SYMBOLS[name], size);

export const fileIcon = (symbol: string, tint: string, size = 14): string =>
  symbolIcon(symbol, size, `background-color:var(--tint-${tint},var(--muted))`);
