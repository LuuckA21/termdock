// Saved layouts: the split tree of a tab (which connection or shell in which
// pane, side by side or stacked, and the sizes) under a name, to open again
// with one click or at startup. Stored in localStorage and validated on every
// read; they hold only connection ids, shell ids and quick-connect addresses,
// never secrets.
import type { SplitDir } from "./tabs";
import type { OpenTarget } from "./types";

export interface LayoutLeaf {
  kind: "pane";
  target: OpenTarget;
  /** Shown if the target cannot be found any more. */
  title: string;
}

interface LayoutSplit {
  kind: "split";
  dir: SplitDir;
  /** Share of the space taken by `a`. */
  ratio: number;
  a: LayoutNode;
  b: LayoutNode;
}

export type LayoutNode = LayoutLeaf | LayoutSplit;

export interface Layout {
  id: string;
  name: string;
  root: LayoutNode;
}

interface Stored {
  layouts: Layout[];
  /** Layout opened at startup; null: the default local shell. */
  startup: string | null;
}

const STORAGE_KEY = "termdock.layouts";
const MAX_LAYOUTS = 50;
export const MAX_PANES = 16;
export const MAX_NAME = 60;
/** A tree of n panes is at most n - 1 deep. */
const MAX_DEPTH = MAX_PANES - 1;
const MAX_TEXT = 255;

const text = (v: unknown, max: number): string | null =>
  typeof v === "string" && v.trim() && v.length <= max ? v.trim() : null;

function target(v: unknown): OpenTarget | null {
  if (!v || typeof v !== "object") return null;
  const o = v as Record<string, unknown>;
  if (o.kind === "profile" || o.kind === "shell") {
    const id = text(o.id, MAX_TEXT);
    return id ? { kind: o.kind, id } : null;
  }
  if (o.kind === "quick") {
    const address = text(o.address, MAX_TEXT);
    return address && !/\s/.test(address) ? { kind: "quick", address } : null;
  }
  return null;
}

/** A valid tree, or null; counts leaves in `budget`. */
function node(v: unknown, depth: number, budget: { panes: number }): LayoutNode | null {
  if (!v || typeof v !== "object" || depth > MAX_DEPTH) return null;
  const o = v as Record<string, unknown>;
  if (o.kind === "pane") {
    const t = target(o.target);
    if (!t || ++budget.panes > MAX_PANES) return null;
    return { kind: "pane", target: t, title: text(o.title, MAX_TEXT) ?? "?" };
  }
  if (o.kind === "split" && (o.dir === "right" || o.dir === "down")) {
    const a = node(o.a, depth + 1, budget);
    const b = node(o.b, depth + 1, budget);
    if (!a || !b) return null;
    const ratio = typeof o.ratio === "number" && Number.isFinite(o.ratio) ? Math.min(0.9, Math.max(0.1, o.ratio)) : 0.5;
    return { kind: "split", dir: o.dir, ratio, a, b };
  }
  return null;
}

function normalize(raw: unknown): Stored {
  const o = raw && typeof raw === "object" ? (raw as Record<string, unknown>) : {};
  const layouts: Layout[] = [];
  for (const item of Array.isArray(o.layouts) ? o.layouts.slice(0, MAX_LAYOUTS) : []) {
    if (!item || typeof item !== "object") continue;
    const l = item as Record<string, unknown>;
    const id = text(l.id, 64);
    const name = text(l.name, MAX_NAME);
    const root = node(l.root, 0, { panes: 0 });
    if (id && name && root && !layouts.some((x) => x.id === id)) layouts.push({ id, name, root });
  }
  const startup = typeof o.startup === "string" && layouts.some((l) => l.id === o.startup) ? o.startup : null;
  return { layouts, startup };
}

function read(): Stored {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return normalize(saved ? JSON.parse(saved) : {});
  } catch {
    return { layouts: [], startup: null };
  }
}

function write(next: Stored): void {
  stored = normalize(next);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(stored));
  } catch {
    // Kept for this run only.
  }
}

let stored = read();

export function getLayouts(): readonly Layout[] {
  return stored.layouts;
}

/** The stored form, for a backup. */
export function storedLayouts(): unknown {
  return stored;
}

/**
 * Layouts from a backup, validated like saved ones: `replace` swaps the whole
 * list (and the startup choice); otherwise layouts not present yet (by id)
 * are added. Returns how many came in.
 */
export function importLayouts(raw: unknown, replace: boolean): number {
  const incoming = normalize(raw);
  if (replace) {
    write(incoming);
    return incoming.layouts.length;
  }
  const added = incoming.layouts.filter((l) => !stored.layouts.some((x) => x.id === l.id));
  write({ ...stored, layouts: [...stored.layouts, ...added] });
  return added.length;
}

export function startupLayout(): Layout | null {
  return stored.layouts.find((l) => l.id === stored.startup) ?? null;
}

export function setStartup(id: string | null): void {
  write({ ...stored, startup: id });
}

/**
 * Saves `root` as `name`, replacing a layout with the same name. Returns null
 * (and changes nothing) if the layout would not pass validation.
 */
export function saveLayout(name: string, root: LayoutNode): Layout | null {
  const checked = node(root, 0, { panes: 0 });
  const cleanName = text(name, MAX_NAME);
  if (!checked || !cleanName) return null;
  const existing = stored.layouts.find((l) => l.name === cleanName);
  const layout: Layout = { id: existing?.id ?? crypto.randomUUID(), name: cleanName, root: checked };
  const layouts = existing
    ? stored.layouts.map((l) => (l.id === existing.id ? layout : l))
    : [...stored.layouts, layout].slice(-MAX_LAYOUTS);
  write({ ...stored, layouts });
  return layout;
}

export function renameLayout(id: string, name: string): void {
  write({ ...stored, layouts: stored.layouts.map((l) => (l.id === id ? { ...l, name } : l)) });
}

export function deleteLayout(id: string): void {
  write({ layouts: stored.layouts.filter((l) => l.id !== id), startup: stored.startup === id ? null : stored.startup });
}

export function paneCount(n: LayoutNode): number {
  return n.kind === "pane" ? 1 : paneCount(n.a) + paneCount(n.b);
}

export function firstLeaf(n: LayoutNode): LayoutLeaf {
  return n.kind === "pane" ? n : firstLeaf(n.a);
}
