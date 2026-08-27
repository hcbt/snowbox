import { afterEach, describe, expect, mock, test } from "bun:test";
import { api, empty, json } from "./api";

const originalFetch = globalThis.fetch;

afterEach(() => {
  globalThis.fetch = originalFetch;
});

function requestUrl(input: RequestInfo | URL): string {
  if (isRequestString(input)) return input;
  if (input instanceof URL) return input.href;
  return input.url;
}

function isRequestString(input: RequestInfo | URL): input is string {
  return typeof input === "string";
}

function stubFetch(handler: (url: string, init?: RequestInit) => Response) {
  const fn = mock(async (input: RequestInfo | URL, init?: RequestInit) => {
    return handler(requestUrl(input), init);
  });
  // SAFETY: bun:test mock matches fetch's callable shape; we restore originalFetch in afterEach.
  globalThis.fetch = fn as typeof fetch;
  return fn;
}

describe("Bearer", () => {
  test("req sends Authorization from window.__SNOWBOX_TOKEN__", async () => {
    // SAFETY: test-only stub of window for the session token read.
    const g = globalThis as typeof globalThis & { window?: Window };
    // SAFETY: test-only Window with the token the Canvas injects.
    g.window = { __SNOWBOX_TOKEN__: "abc123" } as Window;
    const fn = stubFetch((_url, init) => {
      const h = new Headers(init?.headers);
      expect(h.get("authorization")).toBe("Bearer abc123");
      return new Response(JSON.stringify({ ok: true }), {
        status: 200,
        headers: { "content-type": "application/json" },
      });
    });
    await json("/api/v1/health");
    expect(fn).toHaveBeenCalled();
    delete g.window;
  });
});

describe("json / empty", () => {
  test("204 success returns without parsing a body", async () => {
    const fn = stubFetch(() => new Response(null, { status: 204 }));
    await expect(empty("/api/v1/windows/w1", { method: "DELETE" })).resolves.toBeUndefined();
    expect(fn).toHaveBeenCalled();
  });

  test("DELETE !ok throws detail from {error,detail}", async () => {
    stubFetch(
      () =>
        new Response(JSON.stringify({ error: "not_found", detail: "sandbox gone" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    );
    await expect(empty("/api/v1/sandboxes/sb1", { method: "DELETE" })).rejects.toThrow(
      "sandbox gone",
    );
  });

  test("DELETE !ok throws error code when detail is missing", async () => {
    stubFetch(
      () =>
        new Response(JSON.stringify({ error: "conflict" }), {
          status: 409,
          headers: { "content-type": "application/json" },
        }),
    );
    await expect(empty("/x", { method: "DELETE" })).rejects.toThrow("conflict");
  });

  test("DELETE !ok with empty body throws statusText", async () => {
    stubFetch(() => new Response(null, { status: 404, statusText: "Not Found" }));
    await expect(empty("/x", { method: "DELETE" })).rejects.toThrow("Not Found");
  });
});

describe("api.unpublish", () => {
  test("DELETE /sandboxes/{id}/publish/{port}", async () => {
    const fn = stubFetch((url, init) => {
      expect(url).toBe("/api/v1/sandboxes/sb1/publish/3000");
      expect(init?.method).toBe("DELETE");
      return new Response(null, { status: 204 });
    });
    await expect(api.unpublish("sb1", 3000)).resolves.toBeUndefined();
    expect(fn).toHaveBeenCalledTimes(1);
  });

  test("throws on !ok", async () => {
    stubFetch(
      () =>
        new Response(JSON.stringify({ error: "not_found", detail: "port not published" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    );
    await expect(api.unpublish("sb1", 80)).rejects.toThrow("port not published");
  });
});

describe("api.destroy / closeWindow", () => {
  test("destroy throws on DELETE !ok", async () => {
    stubFetch(
      () =>
        new Response(JSON.stringify({ error: "not_found", detail: "no such sandbox" }), {
          status: 404,
          headers: { "content-type": "application/json" },
        }),
    );
    await expect(api.destroy("sb1")).rejects.toThrow("no such sandbox");
  });

  test("closeWindow succeeds on 204", async () => {
    stubFetch((url, init) => {
      expect(url).toBe("/api/v1/windows/w1");
      expect(init?.method).toBe("DELETE");
      return new Response(null, { status: 204 });
    });
    await expect(api.closeWindow("w1")).resolves.toBeUndefined();
  });
});

describe("api.publish", () => {
  test("empty host port sends null", async () => {
    stubFetch((url, init) => {
      expect(url).toBe("/api/v1/sandboxes/sb1/publish");
      expect(JSON.parse(String(init?.body))).toEqual({ port: 3000, host_port: null });
      return new Response(
        JSON.stringify({ port: 3000, host_port: 49152, url: "http://127.0.0.1:49152" }),
        { status: 201, headers: { "content-type": "application/json" } },
      );
    });
    const pub = await api.publish("sb1", 3000);
    expect(pub.url).toBe("http://127.0.0.1:49152");
  });
});
