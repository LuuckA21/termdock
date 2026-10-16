// UI translations. To add a language: create `src/locales/<code>.ts` typed
// as `Dict` (the compiler then requires every key) and register it below.
import en, { type Dict } from "./locales/en";
import it from "./locales/it";

export const LANGUAGES = [
  { code: "en", name: "English", dict: en },
  { code: "it", name: "Italiano", dict: it },
] as const satisfies readonly { code: string; name: string; dict: Dict }[];

export type LanguageCode = (typeof LANGUAGES)[number]["code"];
/** What the user chose: a language, or "auto" to follow the system. */
export type LanguageSetting = LanguageCode | "auto";
export type MsgKey = keyof Dict;

const STORAGE_KEY = "termdock.language";

function isLanguage(value: unknown): value is LanguageCode {
  return LANGUAGES.some((l) => l.code === value);
}

/** First system language Termdock speaks, English otherwise. */
export function systemLanguage(): LanguageCode {
  for (const tag of navigator.languages?.length ? navigator.languages : [navigator.language]) {
    const base = tag.toLowerCase().split("-")[0];
    if (isLanguage(base)) return base;
  }
  return "en";
}

function readSetting(): LanguageSetting {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    if (saved === "auto" || isLanguage(saved)) return saved;
  } catch {
    // Storage unavailable: fall back to the system language.
  }
  return "auto";
}

let setting: LanguageSetting = readSetting();
let current: LanguageCode = "en";
let dict: Dict = en;
const listeners = new Set<() => void>();

function apply(): void {
  current = setting === "auto" ? systemLanguage() : setting;
  dict = LANGUAGES.find((l) => l.code === current)?.dict ?? en;
  document.documentElement.lang = current;
}
apply();

export function languageSetting(): LanguageSetting {
  return setting;
}

export function currentLanguage(): LanguageCode {
  return current;
}

/** Switches language at once and remembers the choice. */
export function setLanguage(next: LanguageSetting): void {
  if (next === setting) return;
  setting = next;
  try {
    localStorage.setItem(STORAGE_KEY, next);
  } catch {
    // Not persisted, but still applied for this run.
  }
  const before = current;
  apply();
  if (current !== before) listeners.forEach((fn) => fn());
}

/** Called after the language changes; returns an unsubscribe function. */
export function onLanguageChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

export function t(key: MsgKey, params: Record<string, string | number> = {}): string {
  return (dict[key] ?? en[key]).replace(/\{(\w+)\}/g, (_, k: string) => String(params[k] ?? `{${k}}`));
}
