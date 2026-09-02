import { describe, expect, test } from "bun:test";
import { applyAppearance, parseMode, parseTheme, termPalette, windowChrome } from "./appearance";

describe("parseTheme", () => {
  test("twm and rio are Themes", () => {
    expect(parseTheme("twm")).toBe("twm");
    expect(parseTheme("rio")).toBe("rio");
  });

  test("anything else is twm", () => {
    expect(parseTheme("plan9")).toBe("twm");
    expect(parseTheme(undefined)).toBe("twm");
  });
});

describe("parseMode", () => {
  test("night and day are Modes", () => {
    expect(parseMode("night")).toBe("night");
    expect(parseMode("day")).toBe("day");
  });

  test("anything else is night", () => {
    expect(parseMode("light")).toBe("night");
    expect(parseMode("dark")).toBe("night");
    expect(parseMode(undefined)).toBe("night");
  });
});

describe("windowChrome", () => {
  test("twm has a titlebar", () => {
    expect(windowChrome("twm").titlebar).toBe(true);
  });

  test("rio has no titlebar", () => {
    expect(windowChrome("rio").titlebar).toBe(false);
  });
});

describe("termPalette", () => {
  test("twm night matches today's xterm", () => {
    expect(termPalette("twm", "night")).toEqual({
      background: "#1c1c20",
      foreground: "#ededef",
      cursor: "#6b3a9e",
      cursorAccent: "#ededef",
      selectionBackground: "#6b3a9e",
      selectionForeground: "#ffffff",
    });
  });

  test("twm day is white body, dark text", () => {
    expect(termPalette("twm", "day")).toEqual({
      background: "#ffffff",
      foreground: "#262626",
      cursor: "#6b3a9e",
      cursorAccent: "#262626",
      selectionBackground: "#6b3a9e",
      selectionForeground: "#ffffff",
    });
  });

  test("rio day is cream", () => {
    expect(termPalette("rio", "day")).toEqual({
      background: "#fff8e0",
      foreground: "#1a1a1a",
      cursor: "#3a9aa8",
      cursorAccent: "#1a1a1a",
      selectionBackground: "#3a9aa8",
      selectionForeground: "#ffffff",
    });
  });

  test("rio night uses the rio night body", () => {
    expect(termPalette("rio", "night")).toEqual({
      background: "#1a1c1a",
      foreground: "#ededef",
      cursor: "#3a9aa8",
      cursorAccent: "#ededef",
      selectionBackground: "#3a9aa8",
      selectionForeground: "#ffffff",
    });
  });
});

describe("applyAppearance", () => {
  test("sets data-theme, data-mode, and color-scheme", () => {
    const attrs: Record<string, string> = {};
    applyAppearance(
      {
        setAttribute: (k, v) => {
          attrs[k] = v;
        },
      },
      "rio",
      "day",
    );
    expect(attrs["data-theme"]).toBe("rio");
    expect(attrs["data-mode"]).toBe("day");
    expect(attrs["color-scheme"]).toBe("light");
  });

  test("night is color-scheme dark", () => {
    const attrs: Record<string, string> = {};
    applyAppearance(
      {
        setAttribute: (k, v) => {
          attrs[k] = v;
        },
      },
      "twm",
      "night",
    );
    expect(attrs["color-scheme"]).toBe("dark");
  });
});
