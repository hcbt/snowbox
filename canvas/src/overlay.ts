export type Overlay =
  | { kind: "sandbox"; x: number; y: number }
  | { kind: "sandboxes"; x: number; y: number }
  | { kind: "limits"; id: string; x: number; y: number }
  | { kind: "environment"; id: string; x: number; y: number }
  | { kind: "templates"; x: number; y: number }
  | { kind: "save-template"; id: string; x: number; y: number }
  | { kind: "publish"; id: string; x: number; y: number }
  | { kind: "copy"; id: string; dir: "in" | "out"; x: number; y: number }
  | { kind: "destroy"; id: string; x: number; y: number }
  | { kind: "reset"; id: string; x: number; y: number };

export type MenuPos = { x: number; y: number };

export const overlayZ = 50000;

export function placeOverlay(x = 220, y = 56) {
  return { x, y } satisfies MenuPos;
}
