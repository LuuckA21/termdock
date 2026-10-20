// Command palette (Ctrl+Shift+A): one filterable list of what the app can
// do right now (actions, connections, shells, layouts, snippets, open tabs),
// built by the caller. Nothing runs until an entry is chosen.
import { modal } from "./dialogs";
import { clear, h, icon, type IconName } from "./dom";
import { t } from "./i18n";

export interface PaletteItem {
  label: string;
  /** What kind of entry it is, and its shortcut or address. */
  detail: string;
  icon: IconName;
  run: () => void;
}

/** Most entries shown at once (the filter narrows the rest). */
const MAX_SHOWN = 200;

/** Every word typed must appear in the label or the detail. */
function matches(item: PaletteItem, words: string[]): boolean {
  const text = `${item.label} ${item.detail}`.toLowerCase();
  return words.every((w) => text.includes(w));
}

/**
 * Shows the palette. `quick` turns typed text that could be an address into
 * a quick-connect entry (the backend checks the address), or null. Resolves
 * when it closes; the chosen entry runs right after, with the dialog gone.
 */
export async function openPalette(items: PaletteItem[], quick: (address: string) => PaletteItem | null): Promise<void> {
  const input = h("input", {
    class: "input",
    spellcheck: "false",
    autocomplete: "off",
    placeholder: t("paletteFilter"),
    "aria-label": t("paletteFilter"),
  });
  const list = h("div", { class: "picker-list", role: "listbox" });
  let shown: PaletteItem[] = [];
  let index = 0;

  const render = () => {
    const words = input.value.trim().toLowerCase().split(/\s+/).filter(Boolean);
    shown = items.filter((i) => matches(i, words)).slice(0, MAX_SHOWN);
    const typed = input.value.trim();
    const address = typed && !/\s/.test(typed) ? quick(typed) : null;
    if (address) shown.push(address);
    index = Math.min(index, Math.max(shown.length - 1, 0));
    clear(list);
    if (!shown.length) list.append(h("p", { class: "muted picker-empty" }, t("paletteNothing")));
    shown.forEach((item, i) => {
      const row = h(
        "div",
        { class: `picker-row${i === index ? " selected" : ""}`, role: "option", "aria-selected": String(i === index) },
        icon(item.icon),
        h("span", { class: "picker-label" }, item.label),
        h("span", { class: "picker-detail muted" }, item.detail),
      );
      row.addEventListener("mousemove", () => {
        if (index !== i) {
          index = i;
          render();
        }
      });
      row.addEventListener("click", () => {
        index = i;
        input.form?.requestSubmit();
      });
      list.append(row);
    });
    list.querySelector(".selected")?.scrollIntoView({ block: "nearest" });
  };

  input.addEventListener("input", () => {
    index = 0;
    render();
  });
  input.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown") index = Math.min(index + 1, shown.length - 1);
    else if (e.key === "ArrowUp") index = Math.max(index - 1, 0);
    else if (e.key === "PageDown") index = Math.min(index + 10, shown.length - 1);
    else if (e.key === "PageUp") index = Math.max(index - 10, 0);
    else return;
    e.preventDefault();
    render();
  });
  render();

  const chosen = await modal<PaletteItem | null>({
    title: t("palette"),
    className: "picker palette",
    body: [input, list],
    buttons: [{ label: t("cancel"), value: null }],
    cancelValue: null,
    initialFocus: input,
    onSubmit: () => shown[index] ?? undefined,
  });
  // After the key that chose it is done: its Enter must not reach a
  // terminal the entry focuses.
  if (chosen) setTimeout(chosen.run, 0);
}
