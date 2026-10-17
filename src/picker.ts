// "What goes in the new pane?": a filterable list shown when splitting, like
// Windows Terminal's profile choice. Enter on the first entry duplicates the
// current pane, so the old one-key behaviour stays.
import { modal } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe } from "./favorites";
import { t } from "./i18n";
import type { DetectedShell, OpenTarget, Profile } from "./types";

export interface PaneChoice {
  title: string;
  subtitle: string;
  target: OpenTarget;
  color?: string;
}

interface Item {
  label: string;
  detail: string;
  icon: "splitRight" | "terminal" | "server" | "bolt";
  /** Lower-case text the filter matches. */
  search: string;
  choice: PaneChoice;
}

export function pickPane(opts: {
  title: string;
  duplicate: PaneChoice | null;
  shells: DetectedShell[];
  profiles: Profile[];
  favorites: string[];
}): Promise<PaneChoice | null> {
  const items: Item[] = [];
  if (opts.duplicate) {
    items.push({
      label: t("pickDuplicate", { name: opts.duplicate.title }),
      detail: opts.duplicate.subtitle,
      icon: "splitRight",
      search: `${opts.duplicate.title} ${opts.duplicate.subtitle}`.toLowerCase(),
      choice: opts.duplicate,
    });
  }
  for (const s of opts.shells) {
    items.push({
      label: s.name,
      detail: t("local"),
      icon: "terminal",
      search: `${s.name} ${s.shell.program}`.toLowerCase(),
      choice: { title: s.name, subtitle: s.shell.program, target: { kind: "shell", id: s.id } },
    });
  }
  // Favourites first, in their order; then the other connections by name.
  const rank = (p: Profile) => {
    const i = opts.favorites.indexOf(p.id);
    return i < 0 ? Number.MAX_SAFE_INTEGER : i;
  };
  const profiles = [...opts.profiles].sort((a, b) => rank(a) - rank(b) || a.name.localeCompare(b.name));
  for (const p of profiles) {
    items.push({
      label: p.name,
      detail: describe(p),
      icon: p.type === "ssh" ? "server" : "terminal",
      search: `${p.name} ${describe(p)}`.toLowerCase(),
      choice: { title: p.name, subtitle: describe(p), target: { kind: "profile", id: p.id }, color: p.color },
    });
  }

  const input = h("input", {
    class: "input",
    spellcheck: "false",
    autocomplete: "off",
    placeholder: t("pickFilter"),
    "aria-label": t("pickFilter"),
  });
  const list = h("div", { class: "picker-list", role: "listbox" });
  let shown: Item[] = [];
  let index = 0;

  const render = () => {
    const q = input.value.trim().toLowerCase();
    shown = q ? items.filter((i) => i.search.includes(q)) : items;
    // Anything typed can also be a quick-connect address (checked by the backend).
    const address = input.value.trim();
    if (address && !/\s/.test(address)) {
      shown = [
        ...shown,
        {
          label: t("pickQuick", { address }),
          detail: t("pickQuickDetail"),
          icon: "bolt",
          search: "",
          choice: { title: address, subtitle: address, target: { kind: "quick", address } },
        },
      ];
    }
    index = Math.min(index, Math.max(shown.length - 1, 0));
    clear(list);
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
    else return;
    e.preventDefault();
    render();
  });
  render();

  return modal<PaneChoice | null>({
    title: opts.title,
    className: "picker",
    body: [input, list],
    buttons: [{ label: t("cancel"), value: null }],
    cancelValue: null,
    initialFocus: input,
    onSubmit: () => shown[index]?.choice,
  });
}
