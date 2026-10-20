// Keyboard shortcuts: the app's actions, their default keys and the user's
// changes (Settings > Keys). Stored in localStorage and validated on every
// read, like the other preferences; an unknown or broken entry falls back to
// the default. "Mod" is Ctrl, or Cmd on macOS (either works everywhere).
import { t, type MsgKey } from "./i18n";

export type Action =
  | "newShell"
  | "closePane"
  | "splitRight"
  | "splitDown"
  | "movePaneLeft"
  | "movePaneRight"
  | "movePaneUp"
  | "movePaneDown"
  | "nextTab"
  | "prevTab"
  | "saveLayout"
  | "quickConnect"
  | "connections"
  | "files"
  | "find"
  | "copy"
  | "paste"
  | "snippets"
  | "clearTerminal"
  | "zoomIn"
  | "zoomOut"
  | "zoomReset"
  | "palette"
  | "settings";

/** Shortcut families over the digits 1…9. */
export type DigitAction = "goToTab" | "openFavorite" | "sendSnippet";
/** Modifiers of a digit family; "off" disables it. */
// No Ctrl+Alt: on Windows, AltGr (needed for @ # [ ] on many layouts) arrives as Ctrl+Alt.
export type DigitMods = "alt" | "mod" | "mod+shift" | "off";
export const DIGIT_MODS: DigitMods[] = ["alt", "mod", "mod+shift", "off"];

/**
 * One key combination. `code` is the physical key (layout independent, used
 * for everything the user records); `key` is a character, used by the font
 * size defaults so "+" works on every layout (Shift is then ignored).
 */
export interface Chord {
  mod: boolean;
  shift: boolean;
  alt: boolean;
  code?: string;
  key?: string;
}

export const ACTION_GROUPS: { label: MsgKey; actions: { id: Action; label: MsgKey }[] }[] = [
  {
    label: "keysGroupTabs",
    actions: [
      { id: "newShell", label: "keyNewShell" },
      { id: "closePane", label: "keyClosePane" },
      { id: "splitRight", label: "splitRight" },
      { id: "splitDown", label: "splitDown" },
      { id: "movePaneLeft", label: "keyMovePaneLeft" },
      { id: "movePaneRight", label: "keyMovePaneRight" },
      { id: "movePaneUp", label: "keyMovePaneUp" },
      { id: "movePaneDown", label: "keyMovePaneDown" },
      { id: "nextTab", label: "keyNextTab" },
      { id: "prevTab", label: "keyPrevTab" },
      { id: "saveLayout", label: "layoutSave" },
    ],
  },
  {
    label: "keysGroupConnections",
    actions: [
      { id: "quickConnect", label: "keyQuickConnect" },
      { id: "connections", label: "connections" },
      { id: "files", label: "openFiles" },
    ],
  },
  {
    label: "keysGroupTerminal",
    actions: [
      { id: "find", label: "find" },
      { id: "copy", label: "copy" },
      { id: "paste", label: "paste" },
      { id: "snippets", label: "keySnippets" },
      { id: "clearTerminal", label: "clearTerminal" },
      { id: "zoomIn", label: "keyZoomIn" },
      { id: "zoomOut", label: "keyZoomOut" },
      { id: "zoomReset", label: "keyZoomReset" },
    ],
  },
  {
    label: "keysGroupApp",
    actions: [
      { id: "palette", label: "palette" },
      { id: "settings", label: "settings" },
    ],
  },
];

export const DIGIT_ACTIONS: { id: DigitAction; label: MsgKey; group: MsgKey }[] = [
  { id: "goToTab", label: "keyGoToTab", group: "keysGroupTabs" },
  { id: "openFavorite", label: "keyOpenFavorite", group: "keysGroupConnections" },
  { id: "sendSnippet", label: "keySendSnippet", group: "keysGroupTerminal" },
];

const ACTIONS: Action[] = ACTION_GROUPS.flatMap((g) => g.actions.map((a) => a.id));

const c = (spec: string): Chord => parseChord(spec) as Chord;

const DEFAULTS: Record<Action, Chord[]> = {
  newShell: [c("mod+shift+KeyT")],
  closePane: [c("mod+shift+KeyW")],
  splitRight: [c("mod+shift+KeyE")],
  splitDown: [c("mod+shift+KeyO")],
  movePaneLeft: [c("alt+ArrowLeft")],
  movePaneRight: [c("alt+ArrowRight")],
  movePaneUp: [c("alt+ArrowUp")],
  movePaneDown: [c("alt+ArrowDown")],
  nextTab: [c("mod+Tab")],
  prevTab: [c("mod+shift+Tab")],
  saveLayout: [c("mod+shift+KeyL")],
  quickConnect: [c("mod+shift+KeyK")],
  connections: [c("mod+shift+KeyM")],
  files: [c("mod+shift+KeyS")],
  find: [c("mod+shift+KeyF")],
  copy: [c("mod+shift+KeyC")],
  paste: [c("mod+shift+KeyV")],
  snippets: [c("mod+shift+KeyP")],
  // None by default: Ctrl+L belongs to the shell (it clears the screen too).
  clearTerminal: [],
  zoomIn: [c("mod+key:+"), c("mod+key:=")],
  zoomOut: [c("mod+key:-")],
  zoomReset: [c("mod+key:0")],
  palette: [c("mod+shift+KeyA")],
  settings: [c("mod+Comma")],
};

export const isMac = /Mac|iPhone|iPad/.test(navigator.userAgent);

// On macOS Option+digit types characters (e.g. [ ] { } on German layouts): Cmd+1…9 there.
const DIGIT_DEFAULTS: Record<DigitAction, DigitMods> = {
  goToTab: isMac ? "mod" : "alt",
  openFavorite: "mod+shift",
  // Off until chosen: every free modifier is already taken by the others.
  sendSnippet: "off",
};

export interface KeyMap {
  actions: Record<Action, Chord[]>;
  digits: Record<DigitAction, DigitMods>;
}

const STORAGE_KEY = "termdock.keys";
const MAX_CHORDS = 4;

// ---- chord text form ("mod+shift+KeyT", "mod+key:+")

export function chordSpec(ch: Chord): string {
  const parts = [ch.mod && "mod", ch.shift && "shift", ch.alt && "alt"].filter(Boolean) as string[];
  parts.push(ch.key !== undefined ? `key:${ch.key}` : (ch.code ?? ""));
  return parts.join("+");
}

/** A chord from its text form, or null if it is not valid. */
function parseChord(spec: unknown): Chord | null {
  if (typeof spec !== "string" || spec.length > 40) return null;
  const keyAt = spec.indexOf("key:");
  const head = keyAt >= 0 ? spec.slice(0, keyAt) : spec.slice(0, spec.lastIndexOf("+") + 1);
  const last = keyAt >= 0 ? spec.slice(keyAt + 4) : spec.slice(spec.lastIndexOf("+") + 1);
  const mods = head.split("+").filter(Boolean);
  if (mods.some((m) => m !== "mod" && m !== "shift" && m !== "alt") || new Set(mods).size !== mods.length) return null;
  const ch: Chord = { mod: mods.includes("mod"), shift: mods.includes("shift"), alt: mods.includes("alt") };
  if (keyAt >= 0) {
    if ([...last].length !== 1) return null;
    ch.key = last;
  } else {
    if (!/^[A-Za-z][A-Za-z0-9]{0,24}$/.test(last)) return null;
    ch.code = last;
  }
  return usable(ch) ? ch : null;
}

/** A chord that cannot be typed by accident: Ctrl/Cmd or Alt, or a function key. */
export function usable(ch: Chord): boolean {
  if (ch.key !== undefined) return ch.mod || ch.alt;
  return ch.mod || ch.alt || /^F([1-9]|1[0-9]|2[0-4])$/.test(ch.code ?? "");
}

function sameChord(a: Chord, b: Chord): boolean {
  return chordSpec(a) === chordSpec(b);
}

// ---- storage

function normalize(raw: unknown): KeyMap {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const savedActions = o.actions && typeof o.actions === "object" ? (o.actions as Record<string, unknown>) : {};
  const savedDigits = o.digits && typeof o.digits === "object" ? (o.digits as Record<string, unknown>) : {};
  const actions = {} as Record<Action, Chord[]>;
  for (const id of ACTIONS) {
    const list = savedActions[id];
    actions[id] = Array.isArray(list)
      ? list
          .slice(0, MAX_CHORDS)
          .map(parseChord)
          .filter((ch): ch is Chord => ch !== null)
      : DEFAULTS[id].map((ch) => ({ ...ch }));
  }
  const digits = {} as Record<DigitAction, DigitMods>;
  for (const { id } of DIGIT_ACTIONS) {
    const v = savedDigits[id];
    digits[id] = DIGIT_MODS.includes(v as DigitMods) ? (v as DigitMods) : DIGIT_DEFAULTS[id];
  }
  return { actions, digits };
}

function read(): KeyMap {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return normalize(saved ? JSON.parse(saved) : {});
  } catch {
    return normalize({});
  }
}

let keymap = read();
const listeners = new Set<() => void>();

export function getKeyMap(): KeyMap {
  return {
    actions: Object.fromEntries(ACTIONS.map((id) => [id, keymap.actions[id].map((ch) => ({ ...ch }))])) as Record<
      Action,
      Chord[]
    >,
    digits: { ...keymap.digits },
  };
}

export function defaultKeyMap(): KeyMap {
  return normalize({});
}

export function isDefault(map: KeyMap, id: Action): boolean {
  const a = map.actions[id];
  const d = DEFAULTS[id];
  return a.length === d.length && a.every((ch, i) => sameChord(ch, d[i]));
}

/** The stored (text) form of a key map, as in localStorage and backups. */
export function storedKeyMap(map: KeyMap = keymap): unknown {
  return {
    version: 1,
    actions: Object.fromEntries(ACTIONS.map((id) => [id, map.actions[id].map(chordSpec)])),
    digits: map.digits,
  };
}

/** The keys a key map in stored form (from a backup) gives to typing snippets ("" if none). */
export function importedSnippetKeys(raw: unknown): string {
  return digitRangeLabel(normalize(raw).digits.sendSnippet);
}

/** Applies a key map in stored form (from a backup), validated like a saved one. */
export function importKeyMap(raw: unknown): void {
  setKeyMap(normalize(raw));
}

export function setKeyMap(next: KeyMap): void {
  const stored = storedKeyMap(next);
  keymap = normalize(stored);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch {
    // Kept for this run only.
  }
  listeners.forEach((fn) => fn());
}

/** Called after the shortcuts change (to refresh hints); returns an unsubscribe function. */
export function onKeysChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

// ---- matching

export function matches(e: KeyboardEvent, ch: Chord): boolean {
  if (ch.mod !== (e.ctrlKey || e.metaKey) || ch.alt !== e.altKey) return false;
  if (ch.key !== undefined) return e.key === ch.key;
  return ch.shift === e.shiftKey && e.code === ch.code;
}

/** The action bound to this key press, if any. */
export function actionFor(e: KeyboardEvent): Action | null {
  return ACTIONS.find((id) => keymap.actions[id].some((ch) => matches(e, ch))) ?? null;
}

/** For Mod/Alt/Shift + 1…9: which family and which digit (1–9). */
export function digitFor(e: KeyboardEvent): { action: DigitAction; n: number } | null {
  const m = /^Digit([1-9])$/.exec(e.code);
  if (!m) return null;
  for (const { id } of DIGIT_ACTIONS) {
    const mods = keymap.digits[id];
    if (mods !== "off" && matches(e, { ...digitChord(mods), code: e.code })) return { action: id, n: Number(m[1]) };
  }
  return null;
}

function digitChord(mods: Exclude<DigitMods, "off">): Chord {
  return { mod: mods.startsWith("mod"), shift: mods.endsWith("shift"), alt: mods === "alt" };
}

/** The chords a digit family takes (empty when off). */
export function digitChords(mods: DigitMods): Chord[] {
  if (mods === "off") return [];
  return Array.from({ length: 9 }, (_, i) => ({ ...digitChord(mods), code: `Digit${i + 1}` }));
}

// ---- recording

/** A chord from a key press in the recorder; null for a lone modifier key. */
export function chordFromEvent(e: KeyboardEvent): Chord | null {
  if (["Shift", "Control", "Alt", "Meta", "AltGraph", "OS"].includes(e.key) || !e.code) return null;
  return { mod: e.ctrlKey || e.metaKey, shift: e.shiftKey, alt: e.altKey, code: e.code };
}

/** True when programs in the terminal also read this key (Ctrl+letter, Alt+letter…). */
export function reachesTerminal(ch: Chord): boolean {
  if (ch.key !== undefined || ch.shift) return false;
  return (ch.mod || ch.alt) && /^(Key[A-Z]|Digit[0-9]|Bracket(Left|Right)|Backslash|Slash|Minus|Equal|Arrow\w+|Space|Backspace)$/.test(ch.code ?? "");
}

/**
 * True for Ctrl+Alt + a character key outside macOS: Windows reports AltGr as
 * Ctrl+Alt, so on Italian, German, Spanish… layouts it could block characters
 * such as @ # € [ ].
 */
export function altGrRisk(ch: Chord): boolean {
  return !isMac && ch.mod && ch.alt && !/^(F\d+|Arrow\w+|Tab|Enter|Escape|Backspace|Delete|Insert|Home|End|Page\w+)$/.test(ch.code ?? "");
}

/** Who else uses `ch` in `map`: an action or a digit family. */
export function owner(map: KeyMap, ch: Chord, except?: Action): { action?: Action; digit?: DigitAction } | null {
  for (const id of ACTIONS) {
    if (id !== except && map.actions[id].some((other) => sameChord(other, ch))) return { action: id };
  }
  for (const { id } of DIGIT_ACTIONS) {
    if (digitChords(map.digits[id]).some((other) => sameChord(other, ch))) return { digit: id };
  }
  return null;
}

// ---- labels

const CODE_LABELS: Record<string, string> = {
  ArrowLeft: "←",
  ArrowRight: "→",
  ArrowUp: "↑",
  ArrowDown: "↓",
  Comma: ",",
  Period: ".",
  Slash: "/",
  Backslash: "\\",
  Semicolon: ";",
  Quote: "'",
  BracketLeft: "[",
  BracketRight: "]",
  Minus: "-",
  Equal: "=",
  Backquote: "`",
  IntlBackslash: "<",
  Escape: "Esc",
  Delete: "Del",
  PageUp: "PgUp",
  PageDown: "PgDn",
};

function codeLabel(code: string): string {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit[0-9]$/.test(code)) return code.slice(5);
  if (/^Numpad[0-9]$/.test(code)) return `Num${code.slice(6)}`;
  return CODE_LABELS[code] ?? code;
}

export function chordLabel(ch: Chord): string {
  const parts: string[] = [];
  if (ch.mod) parts.push(isMac ? "Cmd" : "Ctrl");
  if (ch.alt) parts.push(isMac ? "Option" : "Alt");
  if (ch.shift) parts.push("Shift");
  parts.push(ch.key !== undefined ? ch.key : codeLabel(ch.code ?? ""));
  return parts.join("+");
}

/** The first key of an action, for hints ("" when it has none). */
export function keyHint(id: Action): string {
  const ch = keymap.actions[id][0];
  return ch ? chordLabel(ch) : "";
}

/** "Label (Ctrl+Shift+T)", or just the label when the action has no key. */
export function withKey(label: string, id: Action): string {
  const key = keyHint(id);
  return key ? `${label} (${key})` : label;
}

/** The key of digit `n` in a family ("" when the family is off). */
export function digitHint(id: DigitAction, n: number): string {
  const mods = keymap.digits[id];
  return mods === "off" ? "" : chordLabel({ ...digitChord(mods), code: `Digit${n}` });
}

/** "Alt+1…9", or "" when the family is off. */
export function digitRangeLabel(mods: DigitMods): string {
  return mods === "off" ? "" : `${chordLabel({ ...digitChord(mods), code: "Digit1" })}…9`;
}

/** One line listing every shortcut ("Ctrl+Shift+T New local shell · …"). */
export function shortcutSummary(): string {
  const parts: string[] = [];
  for (const group of ACTION_GROUPS) {
    for (const { id, label } of group.actions) {
      const key = keyHint(id);
      if (key) parts.push(`${key} ${t(label)}`);
    }
  }
  for (const { id, label } of DIGIT_ACTIONS) {
    const range = digitRangeLabel(keymap.digits[id]);
    if (range) parts.push(`${range} ${t(label)}`);
  }
  return parts.join(" · ");
}
