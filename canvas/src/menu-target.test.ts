import { describe, expect, test } from "bun:test";
import type { Sandbox, WindowRec } from "./api";
import { menuHit } from "./menu-target";

const sb = (id: string): Sandbox => ({
  id,
  name: id,
  state: "running",
  home: [".gitconfig"],
  limits: { cpu: 2, ram: 1, disk: 1 },
});

const win = (id: string, sandbox: string): WindowRec => ({
  id,
  sandbox,
  title: id,
  x: 0,
  y: 0,
  w: 1,
  h: 1,
  z: 1,
  iconified: false,
});

const sandboxes = [sb("s1"), sb("s2")];
const windows = [win("w1", "s1")];

describe("menuHit", () => {
  test("empty Canvas has no Window and no Sandbox", () => {
    expect(menuHit(10, 20, {}, windows, sandboxes)).toEqual({
      x: 10,
      y: 20,
      window: null,
      sandbox: null,
    });
  });

  test("a Window under the cursor is that Window and its Sandbox", () => {
    const hit = menuHit(1, 2, { windowId: "w1" }, windows, sandboxes);
    expect(hit.window?.id).toBe("w1");
    expect(hit.sandbox?.id).toBe("s1");
  });

  test("a Sandbox row with no Window is that Sandbox only", () => {
    const hit = menuHit(1, 2, { sandboxId: "s2" }, windows, sandboxes);
    expect(hit.window).toBeNull();
    expect(hit.sandbox?.id).toBe("s2");
  });
});
