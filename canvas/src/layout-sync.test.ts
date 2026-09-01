import { describe, expect, test } from "bun:test";
import type { Layout, WindowRec } from "./api";
import type { Sandbox } from "./api";
import {
  applyFetchedLayout,
  defaultChrome,
  defaultLog,
  loadChrome,
  saveChrome,
  sameLayout,
  sameLines,
  sandboxLive,
} from "./layout-sync";

const win = (over: Partial<WindowRec> & Pick<WindowRec, "id" | "sandbox">): WindowRec => ({
  title: over.id,
  x: 40,
  y: 40,
  w: 640,
  h: 400,
  z: 1,
  iconified: false,
  ...over,
});

const layout = (over: Partial<Layout> = {}): Layout => ({
  windows: [win({ id: "w1", sandbox: "s1", x: 500, y: 80 })],
  icon_manager: { x: 8, y: 8, w: 200, h: 240, visible: true },
  log: defaultLog(),
  ...over,
});

const running = (id: string): Sandbox => ({
  id,
  name: id,
  state: "running",
  home: [],
  limits: { cpu: 2, ram: 1, disk: 1 },
});

describe("applyFetchedLayout", () => {
  test("poll does not snap a Window that is being moved", () => {
    const local = layout();
    const fetched = layout({
      windows: [win({ id: "w1", sandbox: "s1", x: 40, y: 40 })],
    });
    const next = applyFetchedLayout(local, fetched, new Set(["s1"]), true);
    expect(next.windows[0]?.x).toBe(500);
    expect(next.windows[0]?.y).toBe(80);
  });

  test("idle poll takes Host Layout geometry", () => {
    const local = layout();
    const fetched = layout({
      windows: [win({ id: "w1", sandbox: "s1", x: 40, y: 40 })],
    });
    const next = applyFetchedLayout(local, fetched, new Set(["s1"]), false);
    expect(next.windows[0]?.x).toBe(40);
    expect(next.windows[0]?.y).toBe(40);
  });

  test("keeps one Window when the same id is listed twice", () => {
    const fetched = layout({
      windows: [
        win({ id: "w1", sandbox: "s1", title: "this Host — s1 — xterm" }),
        win({ id: "w1", sandbox: "s1", title: "127.0.0.1 — s1 — xterm" }),
      ],
    });
    const next = applyFetchedLayout(layout({ windows: [] }), fetched, new Set(["s1"]), false);
    expect(next.windows).toHaveLength(1);
    expect(next.windows[0]?.id).toBe("w1");
  });

  test("drops Windows whose Sandbox is gone", () => {
    const fetched = layout({
      windows: [win({ id: "w1", sandbox: "gone" }), win({ id: "w2", sandbox: "s1", x: 10, y: 10 })],
    });
    const next = applyFetchedLayout(layout({ windows: [] }), fetched, new Set(["s1"]), false);
    expect(next.windows.map((w) => w.id)).toEqual(["w2"]);
  });

  test("keeps local log Window while moving", () => {
    const local = layout({
      log: { x: 10, y: 20, w: 300, h: 120, visible: true },
    });
    const fetched = layout({
      log: { x: 240, y: 72, w: 560, h: 280, visible: false },
    });
    const next = applyFetchedLayout(local, fetched, new Set(["s1"]), true);
    expect(next.log).toEqual({ x: 10, y: 20, w: 300, h: 120, visible: true });
  });

  test("idle poll restores a log Window that was open", () => {
    const fetched = layout({
      log: { x: 10, y: 20, w: 300, h: 120, visible: true },
    });
    const next = applyFetchedLayout(layout(), fetched, new Set(["s1"]), false);
    expect(next.log.visible).toBe(true);
    expect(next.log.x).toBe(10);
  });

  test("Host Layout without a log field keeps the Canvas log Window", () => {
    const local = layout({
      log: { x: 10, y: 20, w: 300, h: 120, visible: true },
    });
    const fetched = layout();
    delete fetched.log;
    const next = applyFetchedLayout(local, fetched, new Set(["s1"]), false);
    expect(next.log).toEqual({ x: 10, y: 20, w: 300, h: 120, visible: true });
  });
});

describe("sandboxLive", () => {
  test("empty Host list is live so a reload does not flash stopped", () => {
    expect(sandboxLive([], "s1")).toBe(true);
  });

  test("a running Sandbox from the session cache is live on first paint", () => {
    expect(sandboxLive([running("s1")], "s1")).toBe(true);
  });

  test("once the Host list arrives, a missing Sandbox is not live", () => {
    expect(sandboxLive([running("other")], "s1")).toBe(false);
  });
});

describe("sameLines", () => {
  test("equal content is equal even when the array is new", () => {
    expect(sameLines(["a", "b"], ["a", "b"])).toBe(true);
  });

  test("a new line is not equal", () => {
    expect(sameLines(["a"], ["a", "b"])).toBe(false);
  });
});

describe("sameLayout", () => {
  test("true when Windows and overlays match", () => {
    expect(sameLayout(layout(), layout())).toBe(true);
  });

  test("false when a Window moved", () => {
    const a = layout();
    const b = layout({
      windows: [win({ id: "w1", sandbox: "s1", x: 501, y: 80 })],
    });
    expect(sameLayout(a, b)).toBe(false);
  });
});

describe("canvas chrome", () => {
  test("roundtrip Icon Manager and log", () => {
    const store: Record<string, string> = {};
    const chrome = {
      icon_manager: { x: 40, y: 12, w: 200, h: 240, visible: false },
      log: { x: 10, y: 20, w: 560, h: 280, visible: true },
    };
    saveChrome(
      {
        setItem: (k, v) => {
          store[k] = v;
        },
      },
      chrome,
    );
    const loaded = loadChrome({ getItem: (k) => store[k] ?? null });
    expect(loaded.icon_manager.x).toBe(40);
    expect(loaded.log.visible).toBe(true);
  });

  test("missing storage uses defaults", () => {
    expect(loadChrome(undefined)).toEqual(defaultChrome());
  });
});
