export type Status = 'A' | 'M' | 'D' | 'R';
export type LineKind = 'c' | 'a' | 'd';
export type Side = 'old' | 'new';

export type DiffLine = readonly [kind: LineKind, old: number | null, new: number | null, text: string];
export interface Hunk {
  header: string;
  lines: DiffLine[];
}
export interface FileDiff {
  path: string;
  oldPath?: string;
  status: Status;
  isBinary: boolean;
  additions: number;
  deletions: number;
  hunks: Hunk[];
  symbol: string;
  tint: string;
}

export type ThreadAnchor =
  | { kind: 'file'; path: string }
  | { kind: 'line'; path: string; side: Side; line: number }
  | { kind: 'change' };
export type Severity = 'must-fix' | 'suggestion' | 'nit' | 'question';
export interface FindingMeta {
  severity: Severity;
  title: string;
  location?: string;
}
export interface ThreadComment {
  id: number;
  author: string;
  body: string;
  isStreaming: boolean;
}
export interface ReviewThread {
  id: number;
  anchor: ThreadAnchor;
  comments: ThreadComment[];
  finding?: FindingMeta;
}

export interface Theme {
  colors: Record<string, string>;
  isDark: boolean;
  increaseContrast: boolean;
  reduceTransparency: boolean;
}

// Swift calls these through callAsyncJavaScript (HostCommand in PageMessage.swift).
export interface HostApi {
  load(files: FileDiff[], done: boolean): void;
  thread(thread: ReviewThread): void;
  removeThread(id: number): void;
  reveal(path: string): void;
  setInset(points: number): void;
  setTheme(theme: Theme): void;
  fileState(path: string, viewed: boolean, collapsed: boolean): void;
  reset(): void;
  benchScroll(): void;
}

// The page posts these to Swift (PageMessage.init(body:) in PageMessage.swift).
export type PageMessage =
  | { type: 'ready' }
  | { type: 'needMore' }
  | { type: 'firstPaint' }
  | { type: 'visibleFile'; path: string }
  | { type: 'toggleCollapsed'; path: string }
  | { type: 'toggleViewed'; path: string }
  | { type: 'scrollStats'; frames: number; p50: number; p95: number; max: number }
  | { type: 'createThread'; path: string; side?: Side; line?: number }
  | { type: 'post'; thread: number; text: string }
  | { type: 'askClaude'; thread: number }
  | { type: 'remove'; thread: number };

declare global {
  interface Window {
    dp: HostApi;
    webkit: { messageHandlers: { dp: { postMessage(message: PageMessage): void } } };
  }
}

export const send = (message: PageMessage): void => window.webkit.messageHandlers.dp.postMessage(message);

export type Token = [length: number, light: string, dark: string];
export interface HighlightRequest {
  epoch: number;
  f: number;
  path: string;
  oldText: string;
  newText: string;
}
export type HighlightResult =
  | { epoch: number; f: number; none: true }
  | { epoch: number; f: number; none?: undefined; old: Token[][]; new: Token[][] };
