// Appearance preferences: app theme, terminal colours, font, cursor and
// scrollback. Stored in localStorage and validated on every read, so a
// hand-edited or older value falls back to the default instead of breaking
// the terminals.
import { isSchemeId, SCHEMES, type Scheme, type SchemeId } from "./schemes";

export type ThemeSetting = "system" | "dark" | "light";
export type CursorStyle = "block" | "bar" | "underline";

export interface Prefs {
  theme: ThemeSetting;
  /** "auto": Termdock Dark or Light, following the app theme. */
  scheme: SchemeId | "auto";
  /** One font family name; empty means the built-in monospace stack. */
  fontFamily: string;
  fontSize: number;
  lineHeight: number;
  cursorStyle: CursorStyle;
  cursorBlink: boolean;
  scrollback: number;
}

const DEFAULT_FONT_STACK =
  '"Cascadia Mono", "JetBrains Mono", "SF Mono", Menlo, Consolas, "DejaVu Sans Mono", monospace';

export const LIMITS = {
  fontSize: { min: 8, max: 32 },
  lineHeight: { min: 1, max: 2 },
  scrollback: { min: 0, max: 100_000 },
  fontFamilyLength: 100,
} as const;

export const DEFAULTS: Prefs = {
  theme: "dark",
  scheme: "auto",
  fontFamily: "",
  fontSize: 14,
  lineHeight: 1.1,
  cursorStyle: "block",
  cursorBlink: true,
  scrollback: 10_000,
};

const STORAGE_KEY = "termdock.prefs";

function clamp(value: unknown, min: number, max: number, fallback: number, step = 1): number {
  // Number("") is 0: an emptied field must not become the minimum.
  if (typeof value === "string" ? !value.trim() : typeof value !== "number") return fallback;
  const n = Number(value);
  if (!Number.isFinite(n)) return fallback;
  return Number(Math.min(max, Math.max(min, Math.round(n / step) * step)).toFixed(2));
}

/** A single family name: no quotes, separators or control characters. */
function cleanFontFamily(value: unknown): string {
  if (typeof value !== "string") return "";
  return value
    .replace(/[\p{Cc}"'`;,{}()<>\\]/gu, "")
    .trim()
    .slice(0, LIMITS.fontFamilyLength);
}

export function normalize(raw: unknown): Prefs {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  return {
    theme: o.theme === "dark" || o.theme === "light" || o.theme === "system" ? o.theme : DEFAULTS.theme,
    scheme: o.scheme === "auto" || isSchemeId(o.scheme) ? o.scheme : DEFAULTS.scheme,
    fontFamily: cleanFontFamily(o.fontFamily),
    fontSize: clamp(o.fontSize, LIMITS.fontSize.min, LIMITS.fontSize.max, DEFAULTS.fontSize),
    lineHeight: clamp(o.lineHeight, LIMITS.lineHeight.min, LIMITS.lineHeight.max, DEFAULTS.lineHeight, 0.05),
    cursorStyle:
      o.cursorStyle === "bar" || o.cursorStyle === "underline" || o.cursorStyle === "block"
        ? o.cursorStyle
        : DEFAULTS.cursorStyle,
    cursorBlink: typeof o.cursorBlink === "boolean" ? o.cursorBlink : DEFAULTS.cursorBlink,
    scrollback: clamp(o.scrollback, LIMITS.scrollback.min, LIMITS.scrollback.max, DEFAULTS.scrollback),
  };
}

function read(): Prefs {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return normalize(saved ? JSON.parse(saved) : {});
  } catch {
    return { ...DEFAULTS };
  }
}

let prefs = read();
const listeners = new Set<() => void>();
const systemDark = window.matchMedia("(prefers-color-scheme: dark)");

export function getPrefs(): Readonly<Prefs> {
  return prefs;
}

/** Applies and remembers new preferences (validated). */
export function setPrefs(next: Partial<Prefs>): void {
  prefs = normalize({ ...prefs, ...next });
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(prefs));
  } catch {
    // Not persisted, but still applied for this run.
  }
  applyTheme();
  listeners.forEach((fn) => fn());
}

/** Called after the preferences (or the system theme) change; returns an unsubscribe function. */
export function onPrefsChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

function isDarkTheme(p: Prefs = prefs): boolean {
  return p.theme === "system" ? systemDark.matches : p.theme === "dark";
}

export function terminalScheme(p: Prefs = prefs): Scheme {
  if (p.scheme !== "auto") return SCHEMES[p.scheme];
  return isDarkTheme(p) ? SCHEMES["termdock-dark"] : SCHEMES["termdock-light"];
}

export function fontStack(p: Prefs = prefs): string {
  return p.fontFamily ? `"${p.fontFamily}", ${DEFAULT_FONT_STACK}` : DEFAULT_FONT_STACK;
}

/**
 * App chrome colours follow the theme; the terminal area (and the active tab
 * joined to it) follows the colour scheme, which may be light on a dark theme.
 */
function applyTheme(): void {
  const root = document.documentElement;
  const { colors } = terminalScheme();
  root.dataset.theme = isDarkTheme() ? "dark" : "light";
  root.style.setProperty("--bg-term", colors.background ?? "");
  root.style.setProperty("--fg-term", colors.foreground ?? "");
}

systemDark.addEventListener("change", () => {
  if (prefs.theme !== "system") return;
  applyTheme();
  listeners.forEach((fn) => fn());
});
applyTheme();
