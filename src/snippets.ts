// Snippets: saved texts typed into the active terminal, from the Snippets
// menu or a 1…9 shortcut. Stored in localStorage and validated on every read,
// like the other preferences. They are plain text on this computer, so they
// must never hold passwords (Settings says so).

export interface Snippet {
  id: string;
  name: string;
  text: string;
  /** Press Enter after the text, so the command runs. */
  run: boolean;
  /** Offered only for connections of this group; "" for every terminal. */
  group: string;
}

const STORAGE_KEY = "termdock.snippets";
export const MAX_SNIPPETS = 100;
export const MAX_NAME = 60;
export const MAX_TEXT = 16 * 1024;
const MAX_GROUP = 100;

function normalize(raw: unknown): Snippet[] {
  const out: Snippet[] = [];
  for (const item of Array.isArray(raw) ? raw.slice(0, MAX_SNIPPETS) : []) {
    if (!item || typeof item !== "object") continue;
    const o = item as Record<string, unknown>;
    const id = typeof o.id === "string" && o.id.length <= 64 ? o.id : "";
    const name = typeof o.name === "string" ? o.name.trim() : "";
    const text = typeof o.text === "string" ? o.text : "";
    const group = typeof o.group === "string" && o.group.length <= MAX_GROUP ? o.group.trim() : "";
    if (!id || !name || name.length > MAX_NAME || !text || text.length > MAX_TEXT || out.some((s) => s.id === id)) continue;
    out.push({ id, name, text, run: o.run === true, group });
  }
  return out;
}

function read(): Snippet[] {
  try {
    const saved = localStorage.getItem(STORAGE_KEY);
    return normalize(saved ? JSON.parse(saved) : []);
  } catch {
    return [];
  }
}

let snippets = read();
const listeners = new Set<() => void>();

export function getSnippets(): readonly Snippet[] {
  return snippets;
}

/** A copy to edit (Settings saves it back with `setSnippets`). */
export function copySnippets(): Snippet[] {
  return snippets.map((s) => ({ ...s }));
}

export function setSnippets(next: Snippet[]): void {
  snippets = normalize(next);
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(snippets));
  } catch {
    // Kept for this run only.
  }
  listeners.forEach((fn) => fn());
}

/**
 * Snippets from a backup, validated like saved ones: `replace` swaps the
 * whole list; otherwise snippets not present yet (by id) are added. Returns
 * how many came in.
 */
export function importSnippets(raw: unknown, replace: boolean): number {
  const incoming = normalize(raw);
  if (replace) {
    setSnippets(incoming);
    return incoming.length;
  }
  const added = incoming.filter((s) => !snippets.some((x) => x.id === s.id));
  setSnippets([...snippets, ...added]);
  return added.length;
}

/** Called after the snippets change; returns an unsubscribe function. */
export function onSnippetsChange(fn: () => void): () => void {
  listeners.add(fn);
  return () => listeners.delete(fn);
}

/** Whether `s` is offered in a terminal of `group` (null: no saved connection). */
export function fitsGroup(s: Snippet, group: string | null): boolean {
  return !s.group || s.group === group;
}
