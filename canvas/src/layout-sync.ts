import type { Layout, LogRec, Sandbox, WindowRec } from "./api";

export function defaultLog(): LogRec {
  return { x: 240, y: 72, w: 560, h: 280, visible: false };
}

export function defaultLayout(): Layout {
  return {
    windows: [],
    icon_manager: { x: 8, y: 8, w: 200, h: 240, visible: true },
    log: defaultLog(),
  };
}

function clampSize(n: number, fallback: number): number {
  return n > 0 ? n : fallback;
}

function normalizeLog(log: LogRec | undefined): LogRec {
  const l = log ?? defaultLog();
  return {
    x: l.x,
    y: l.y,
    w: clampSize(l.w, 560),
    h: clampSize(l.h, 280),
    visible: l.visible,
  };
}

function normalizeIcon(im: Layout["icon_manager"]): Layout["icon_manager"] {
  return {
    x: im.x,
    y: im.y,
    w: clampSize(im.w, 200),
    h: clampSize(im.h, 240),
    visible: im.visible,
  };
}

export function normalizeLayout(fetched: Layout, sandboxIds: Set<string>): Layout {
  return {
    windows: fetched.windows.filter((w) => sandboxIds.has(w.sandbox)),
    icon_manager: normalizeIcon(fetched.icon_manager),
    log: normalizeLog(fetched.log),
  };
}

export const CHROME_KEY = "snowbox.chrome";

export type CanvasChrome = {
  icon_manager: Layout["icon_manager"];
  log: LogRec;
};

export function defaultChrome(): CanvasChrome {
  return {
    icon_manager: defaultLayout().icon_manager,
    log: defaultLog(),
  };
}

export function loadChrome(
  storage: Pick<Storage, "getItem"> | undefined,
  fallback?: CanvasChrome,
): CanvasChrome {
  const seed = fallback ?? defaultChrome();
  if (!storage) return seed;
  try {
    const raw = storage.getItem(CHROME_KEY);
    if (!raw) return seed;
    // SAFETY: saveChrome writes CanvasChrome.
    const parsed = JSON.parse(raw) as CanvasChrome;
    return {
      icon_manager: normalizeIcon(parsed.icon_manager ?? seed.icon_manager),
      log: normalizeLog(parsed.log ?? seed.log),
    };
  } catch {
    return seed;
  }
}

export function saveChrome(storage: Pick<Storage, "setItem">, chrome: CanvasChrome): void {
  storage.setItem(
    CHROME_KEY,
    JSON.stringify({
      icon_manager: normalizeIcon(chrome.icon_manager),
      log: normalizeLog(chrome.log),
    }),
  );
}

const SNAP_CACHE = "snowbox.canvas";
const LEGACY_LAYOUT_CACHE = "snowbox.layout";

export type TermPoster = { id: string; html: string };

export type CanvasSnap = {
  layout: Layout;
  sandboxes: Sandbox[];
  logLines: string[];
  termPosters: TermPoster[];
};

export function sandboxLive(sandboxes: Sandbox[], id: string): boolean {
  const row = sandboxes.find((s) => s.id === id);
  if (!row) return sandboxes.length === 0;
  return row.state === "running";
}

export function sameLines(a: string[], b: string[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i] !== b[i]) return false;
  }
  return true;
}

export function sameSandboxes(a: Sandbox[], b: Sandbox[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    const left = a[i];
    const right = b[i];
    if (!left || !right) return false;
    if (
      left.id !== right.id ||
      left.host !== right.host ||
      left.state !== right.state ||
      left.booting !== right.booting
    ) {
      return false;
    }
  }
  return true;
}

function emptySnap(): CanvasSnap {
  return { layout: defaultLayout(), sandboxes: [], logLines: [], termPosters: [] };
}

function readSnapRaw(): CanvasSnap | null {
  try {
    const store = globalThis.sessionStorage;
    if (!store) return null;
    const raw = store.getItem(SNAP_CACHE);
    if (raw) {
      // SAFETY: sessionStorage holds JSON this Canvas wrote as CanvasSnap.
      const parsed = JSON.parse(raw) as CanvasSnap;
      if (!parsed.layout || !Array.isArray(parsed.layout.windows)) return null;
      return {
        layout: {
          windows: parsed.layout.windows,
          icon_manager: normalizeIcon(parsed.layout.icon_manager),
          log: normalizeLog(parsed.layout.log),
        },
        sandboxes: Array.isArray(parsed.sandboxes) ? parsed.sandboxes : [],
        logLines: Array.isArray(parsed.logLines) ? parsed.logLines : [],
        termPosters: Array.isArray(parsed.termPosters) ? parsed.termPosters : [],
      };
    }
    const legacy = store.getItem(LEGACY_LAYOUT_CACHE);
    if (!legacy) return null;
    // SAFETY: older sessions stored Layout JSON under snowbox.layout.
    const layout = JSON.parse(legacy) as Layout;
    if (!Array.isArray(layout.windows) || layout.icon_manager == null) return null;
    return {
      layout: {
        windows: layout.windows,
        icon_manager: normalizeIcon(layout.icon_manager),
        log: normalizeLog(layout.log),
      },
      sandboxes: [],
      logLines: [],
      termPosters: [],
    };
  } catch {
    return null;
  }
}

export function readCachedSnap(): CanvasSnap | null {
  return readSnapRaw();
}

export function readCachedLayout(): Layout | null {
  return readSnapRaw()?.layout ?? null;
}

function writeSnap(snap: CanvasSnap): void {
  try {
    globalThis.sessionStorage?.setItem(SNAP_CACHE, JSON.stringify(snap));
  } catch {
    /* private mode / missing storage */
  }
}

export function writeCachedLayout(layout: Layout): void {
  const cur = readSnapRaw() ?? emptySnap();
  writeSnap({ ...cur, layout });
}

export function writeCachedSandboxes(sandboxes: Sandbox[]): void {
  const cur = readSnapRaw() ?? emptySnap();
  writeSnap({ ...cur, sandboxes });
}

export function writeCachedLogLines(logLines: string[]): void {
  const cur = readSnapRaw() ?? emptySnap();
  writeSnap({ ...cur, logLines });
}

export function readTermPoster(id: string): string | undefined {
  return readSnapRaw()?.termPosters.find((p) => p.id === id)?.html;
}

export function writeTermPoster(id: string, html: string): void {
  const cur = readSnapRaw() ?? emptySnap();
  writeSnap({
    ...cur,
    termPosters: [...cur.termPosters.filter((p) => p.id !== id), { id, html }],
  });
}

function frozenWindow(remote: WindowRec, local: WindowRec | undefined): WindowRec {
  if (!local) return remote;
  return {
    ...remote,
    x: local.x,
    y: local.y,
    w: local.w,
    h: local.h,
    z: local.z,
    iconified: local.iconified,
  };
}

/** Host Layout, except in-flight geometry stays on the Canvas while a Frame is moved. */
export function applyFetchedLayout(
  local: Layout,
  fetched: Layout,
  sandboxIds: Set<string>,
  freezeGeometry: boolean,
): Layout {
  const remote = normalizeLayout(fetched, sandboxIds);
  if (!freezeGeometry) {
    return { ...remote, log: normalizeLog(fetched.log ?? local.log) };
  }
  const localById = new Map(local.windows.map((w) => [w.id, w]));
  return {
    windows: remote.windows.map((w) => frozenWindow(w, localById.get(w.id))),
    icon_manager: local.icon_manager,
    log: normalizeLog(local.log),
  };
}

export function sameLayout(a: Layout, b: Layout): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}
