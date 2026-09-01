import { describe, expect, test } from "bun:test";
import {
  applyDiscovery,
  loadRoster,
  normalizeUrl,
  removeHost,
  saveRoster,
  upsertHost,
  type HostRec,
} from "./hosts";

const a: HostRec = {
  id: "11111111-1111-1111-1111-111111111111",
  url: "http://10.0.0.2:5418",
  token: "tok-a",
  label: "10.0.0.2",
};

describe("normalizeUrl", () => {
  test("adds http and default port", () => {
    expect(normalizeUrl("10.0.0.2")).toBe("http://10.0.0.2:5418");
    expect(normalizeUrl("http://10.0.0.2:5418/")).toBe("http://10.0.0.2:5418");
  });
});

describe("roster", () => {
  test("upsert replaces the same Host id", () => {
    const next = upsertHost([a], { ...a, url: "http://10.0.0.9:5418" });
    expect(next).toHaveLength(1);
    expect(next[0]?.url).toBe("http://10.0.0.9:5418");
  });

  test("remove is Detach", () => {
    expect(removeHost([a], a.id)).toEqual([]);
  });

  test("roundtrip storage", () => {
    const store: Record<string, string> = {};
    saveRoster(
      {
        setItem: (k, v) => {
          store[k] = v;
        },
      },
      [a],
    );
    const loaded = loadRoster({ getItem: (k) => store[k] ?? null });
    expect(loaded).toEqual([a]);
  });
});

describe("applyDiscovery", () => {
  test("updates address of already Attached Hosts only", () => {
    const next = applyDiscovery(
      [a],
      [
        { id: a.id, addresses: ["192.168.1.9"], port: 5418 },
        { id: "99999999-9999-9999-9999-999999999999", addresses: ["1.2.3.4"], port: 5418 },
      ],
    );
    expect(next[0]?.url).toBe("http://192.168.1.9:5418");
    expect(next).toHaveLength(1);
  });
});
