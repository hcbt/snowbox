import { describe, expect, test } from "bun:test";
import type { Sandbox, WindowRec } from "./api";
import { sandboxesWithoutWindows } from "./sandbox-icons";

const sb = (id: string, name: string, state: Sandbox["state"]): Sandbox => ({
  id,
  name,
  state,
  home: [".gitconfig"],
  limits: { cpu: 2, ram: 2147483648, disk: 8589934592 },
});

const win = (sandbox: string): WindowRec => ({
  id: `w-${sandbox}`,
  sandbox,
  title: `${sandbox} — xterm`,
  x: 0,
  y: 0,
  w: 640,
  h: 400,
  z: 1,
  iconified: false,
});

describe("sandboxesWithoutWindows", () => {
  test("API list of 4 with 2 windows leaves the other 2", () => {
    const sandboxes = [
      sb("a", "running-a", "running"),
      sb("b", "running-b", "running"),
      sb("c", "stopped-c", "stopped"),
      sb("d", "stopped-d", "stopped"),
    ];
    const hidden = sandboxesWithoutWindows(sandboxes, [win("a"), win("b")]);
    expect(hidden.map((s) => s.id)).toEqual(["c", "d"]);
  });

  test("every sandbox with a Window is visible", () => {
    const sandboxes = [sb("a", "a", "stopped")];
    expect(sandboxesWithoutWindows(sandboxes, [win("a")])).toEqual([]);
  });
});
