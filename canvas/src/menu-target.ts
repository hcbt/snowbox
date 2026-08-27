import type { Sandbox, WindowRec } from "./api";

export type MenuHit = {
  x: number;
  y: number;
  window: WindowRec | null;
  sandbox: Sandbox | null;
};

/** Context menu acts on the thing under the cursor, not last PTY focus. */
export function menuHit(
  x: number,
  y: number,
  spec: { windowId?: string; sandboxId?: string },
  windows: WindowRec[],
  sandboxes: Sandbox[],
): MenuHit {
  const win = spec.windowId ? (windows.find((w) => w.id === spec.windowId) ?? null) : null;
  if (win) {
    return {
      x,
      y,
      window: win,
      sandbox: sandboxes.find((s) => s.id === win.sandbox) ?? null,
    };
  }
  if (spec.sandboxId) {
    return {
      x,
      y,
      window: null,
      sandbox: sandboxes.find((s) => s.id === spec.sandboxId) ?? null,
    };
  }
  return { x, y, window: null, sandbox: null };
}
