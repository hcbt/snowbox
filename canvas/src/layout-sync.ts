import type { Layout, LogRec, WindowRec } from "./api";

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

const LAYOUT_CACHE = "snowbox.layout";

export function readCachedLayout(): Layout | null {
  try {
    const raw = globalThis.sessionStorage?.getItem(LAYOUT_CACHE);
    if (!raw) return null;
    // SAFETY: sessionStorage holds JSON this Canvas wrote as Layout.
    const parsed = JSON.parse(raw) as Layout;
    if (!Array.isArray(parsed.windows) || parsed.icon_manager == null) return null;
    return {
      windows: parsed.windows,
      icon_manager: normalizeIcon(parsed.icon_manager),
      log: normalizeLog(parsed.log),
    };
  } catch {
    return null;
  }
}

export function writeCachedLayout(layout: Layout): void {
  try {
    globalThis.sessionStorage?.setItem(LAYOUT_CACHE, JSON.stringify(layout));
  } catch {
    /* private mode / missing storage */
  }
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
