import type { Sandbox, WindowRec } from "./api";

/** Sandboxes the Canvas would not draw, because Icon Manager and Frames are Windows. */
export function sandboxesWithoutWindows(sandboxes: Sandbox[], windows: WindowRec[]): Sandbox[] {
  const withWin = new Set(windows.map((w) => w.sandbox));
  return sandboxes.filter((s) => !withWin.has(s.id));
}
