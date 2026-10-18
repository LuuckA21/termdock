// Tiny DOM helpers. Text always goes through textContent, never innerHTML,
// so profile names or server banners cannot inject markup.

/** Control, format (bidi, zero-width, BOM, tags…) and line/paragraph separators. */
const UNSAFE = /[\p{Cc}\p{Cf}\p{Zl}\p{Zp}]/gu;

/**
 * Server-provided text made safe to display: invisible and reordering
 * characters become U+FFFD (no `evil\u202Egpj.exe` spoofing). `keepNewlines`
 * keeps "\n" for multi-line text such as login instructions.
 */
export function displayName(text: string, keepNewlines = false): string {
  return text.replace(UNSAFE, (c) => (keepNewlines && c === "\n" ? c : "\ufffd"));
}

type Attrs = Record<string, string | number | boolean | null | undefined | EventListener>;
type Child = Node | string | null | undefined | false;

export function h<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  attrs: Attrs = {},
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const el = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v === null || v === undefined || v === false) continue;
    if (k.startsWith("on") && typeof v === "function") {
      el.addEventListener(k.slice(2).toLowerCase(), v);
    } else if (k === "class") {
      el.className = String(v);
    } else if (v === true) {
      el.setAttribute(k, "");
    } else {
      el.setAttribute(k, String(v));
    }
  }
  for (const c of children) {
    if (c === null || c === undefined || c === false) continue;
    el.append(typeof c === "string" ? document.createTextNode(c) : c);
  }
  return el;
}

const SVG_NS = "http://www.w3.org/2000/svg";

/** Builds a 16px stroke icon from static path data (no innerHTML). */
export type IconName = keyof typeof ICONS;

export function icon(name: IconName): SVGSVGElement {
  const svg = document.createElementNS(SVG_NS, "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", "16");
  svg.setAttribute("height", "16");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("icon");
  for (const d of ICONS[name]) {
    const p = document.createElementNS(SVG_NS, "path");
    p.setAttribute("d", d);
    svg.append(p);
  }
  return svg;
}

const ICONS = {
  plus: ["M12 5v14", "M5 12h14"],
  close: ["M6 6l12 12", "M18 6L6 18"],
  star: ["M12 3l2.8 5.7 6.2.9-4.5 4.4 1 6.2L12 17.3 6.5 20.2l1-6.2L3 9.6l6.2-.9z"],
  terminal: ["M4 17l6-5-6-5", "M12 19h8"],
  server: ["M4 4h16v6H4z", "M4 14h16v6H4z", "M8 7h.01", "M8 17h.01"],
  list: ["M8 6h13", "M8 12h13", "M8 18h13", "M3 6h.01", "M3 12h.01", "M3 18h.01"],
  edit: ["M4 20h4L19 9l-4-4L4 16z", "M14 6l4 4"],
  trash: ["M4 7h16", "M10 11v6", "M14 11v6", "M6 7l1 13h10l1-13", "M9 7V4h6v3"],
  chevron: ["M6 9l6 6 6-6"],
  bolt: ["M13 2L4 14h7l-1 8 9-12h-7z"],
  keyboard: ["M3 6h18v12H3z", "M7 10h.01", "M11 10h.01", "M15 10h.01", "M7 14h10"],
  download: ["M12 4v11", "M7 10l5 5 5-5", "M5 20h14"],
  splitRight: ["M4 5h16v14H4z", "M12 5v14"],
  folder: ["M3 6h6l2 2h10v11H3z"],
  user: ["M12 12a4 4 0 1 0 0-8 4 4 0 0 0 0 8z", "M4 21a8 8 0 0 1 16 0"],
  folderPlus: ["M3 6h6l2 2h10v11H3z", "M12 11v6", "M9 14h6"],
  file: ["M6 3h8l4 4v14H6z", "M14 3v4h4"],
  link: ["M10 14a4 4 0 0 0 6 0l3-3a4 4 0 0 0-6-6l-1 1", "M14 10a4 4 0 0 0-6 0l-3 3a4 4 0 0 0 6 6l1-1"],
  upload: ["M12 20V9", "M7 14l5-5 5 5", "M5 4h14"],
  arrowUp: ["M12 19V5", "M6 11l6-6 6 6"],
  home: ["M3 11l9-7 9 7", "M5 10v10h14V10"],
  refresh: ["M20 11a8 8 0 1 0-2.3 5.7", "M20 4v7h-7"],
  splitDown: ["M4 5h16v14H4z", "M4 12h16"],
  lock: ["M6 11h12v9H6z", "M8 11V8a4 4 0 0 1 8 0v3"],
  palette: [
    "M12 3a9 9 0 1 0 0 18c1.1 0 1.7-.8 1.7-1.7 0-.5-.2-.9-.5-1.2-.3-.3-.5-.7-.5-1.2 0-.9.8-1.7 1.7-1.7H16a5 5 0 0 0 5-5c0-4-4-7.2-9-7.2z",
    "M7.5 11.5h.01",
    "M10.5 7.5h.01",
    "M15.5 8.5h.01",
  ],
  settings: [
    "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6z",
    "M19.4 15a1.7 1.7 0 0 0 .3 1.8l.1.1a2 2 0 1 1-2.8 2.8l-.1-.1a1.7 1.7 0 0 0-1.8-.3 1.7 1.7 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.7 1.7 0 0 0-1.1-1.5 1.7 1.7 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.7 1.7 0 0 0 .3-1.8 1.7 1.7 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.7 1.7 0 0 0 1.5-1.1 1.7 1.7 0 0 0-.3-1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.7 1.7 0 0 0 1.8.3H9a1.7 1.7 0 0 0 1-1.5V3a2 2 0 1 1 4 0v.1a1.7 1.7 0 0 0 1 1.5 1.7 1.7 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1.1a1.7 1.7 0 0 0-.3 1.8V9a1.7 1.7 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.7 1.7 0 0 0-1.5 1z",
  ],
} as const;

export function clear(el: Element): void {
  while (el.firstChild) el.firstChild.remove();
}

/** A tooltip that is also the accessible name (icon buttons). */
export function setLabel(el: HTMLElement, text: string): void {
  el.title = text;
  el.setAttribute("aria-label", text);
}
