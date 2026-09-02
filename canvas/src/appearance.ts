export type Theme = "twm" | "rio";
export type Mode = "night" | "day";

export const defaultTheme: Theme = "twm";
export const defaultMode: Mode = "night";

export function parseTheme(value: string | undefined): Theme {
  return value === "rio" ? "rio" : defaultTheme;
}

export function parseMode(value: string | undefined): Mode {
  return value === "day" ? "day" : defaultMode;
}

export type WindowChrome = {
  titlebar: boolean;
};

export function windowChrome(theme: Theme): WindowChrome {
  return { titlebar: theme === "twm" };
}

export type TermPalette = {
  background: string;
  foreground: string;
  cursor: string;
  cursorAccent: string;
  selectionBackground: string;
  selectionForeground: string;
};

export function termPalette(theme: Theme, mode: Mode): TermPalette {
  if (theme === "rio" && mode === "day") {
    return {
      background: "#fff8e0",
      foreground: "#1a1a1a",
      cursor: "#3a9aa8",
      cursorAccent: "#1a1a1a",
      selectionBackground: "#3a9aa8",
      selectionForeground: "#ffffff",
    };
  }
  if (theme === "rio") {
    return {
      background: "#1a1c1a",
      foreground: "#ededef",
      cursor: "#3a9aa8",
      cursorAccent: "#ededef",
      selectionBackground: "#3a9aa8",
      selectionForeground: "#ffffff",
    };
  }
  if (mode === "day") {
    return {
      background: "#ffffff",
      foreground: "#262626",
      cursor: "#6b3a9e",
      cursorAccent: "#262626",
      selectionBackground: "#6b3a9e",
      selectionForeground: "#ffffff",
    };
  }
  return {
    background: "#1c1c20",
    foreground: "#ededef",
    cursor: "#6b3a9e",
    cursorAccent: "#ededef",
    selectionBackground: "#6b3a9e",
    selectionForeground: "#ffffff",
  };
}

export function applyAppearance(
  el: { setAttribute: (name: string, value: string) => void },
  theme: Theme,
  mode: Mode,
): void {
  el.setAttribute("data-theme", theme);
  el.setAttribute("data-mode", mode);
  el.setAttribute("color-scheme", mode === "day" ? "light" : "dark");
}
