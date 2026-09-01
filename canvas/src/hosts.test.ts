import { describe, expect, test } from "bun:test";
import {
  applyDiscovery,
  ROSTER_KEY,
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

  test("upsert collapses the same URL even when ids differ", () => {
    const stub: HostRec = {
      id: "origin",
      url: "http://127.0.0.1:5418",
      token: "tok-loop",
      label: "this Host",
    };
    const real: HostRec = {
      id: "0fd95f05-0b62-43c6-89e1-a22a6d69efed",
      url: "http://127.0.0.1:5418",
      token: "tok-loop",
      label: "127.0.0.1",
    };
    const next = upsertHost([stub], real);
    expect(next).toHaveLength(1);
    expect(next[0]?.id).toBe(real.id);
    expect(next[0]?.label).toBe("127.0.0.1");
  });

  test("loadRoster collapses leftover same-URL rows", () => {
    const store: Record<string, string> = {};
    store[ROSTER_KEY] = JSON.stringify([
      {
        id: "origin",
        url: "http://127.0.0.1:5418",
        token: "tok",
        label: "this Host",
      },
      {
        id: "0fd95f05-0b62-43c6-89e1-a22a6d69efed",
        url: "http://127.0.0.1:5418/",
        token: "tok",
        label: "127.0.0.1",
      },
    ]);
    const loaded = loadRoster({ getItem: (k) => store[k] ?? null });
    expect(loaded).toHaveLength(1);
    expect(loaded[0]?.id).toBe("0fd95f05-0b62-43c6-89e1-a22a6d69efed");
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
