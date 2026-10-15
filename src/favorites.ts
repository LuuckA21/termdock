import { h, icon, clear } from "./dom";
import { t } from "./i18n";
import { showMenu, showMenuBelow } from "./menu";
import type { App } from "./main";
import type { Profile } from "./types";

/** Colours come from validated profiles, but are re-checked before use. */
export function setDotColor(dot: HTMLElement, color: string | undefined): void {
  if (color && /^#[0-9a-fA-F]{6}$/.test(color)) dot.style.background = color;
}

export function describe(p: Profile): string {
  if (p.type === "local") return [p.program, ...p.args].join(" ");
  const host = p.host.includes(":") ? `[${p.host}]` : p.host;
  return `${p.username}@${host}${p.port !== 22 ? `:${p.port}` : ""}`;
}

/** The bar at the top: pinned connections, quick connect and shortcuts. */
export class FavoritesBar {
  private dragId: string | null = null;

  constructor(
    private readonly root: HTMLElement,
    private readonly app: App,
  ) {}

  render(): void {
    clear(this.root);
    const chips = h("div", { class: "fav-chips" });
    chips.addEventListener(
      "wheel",
      (e) => {
        if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) chips.scrollLeft += e.deltaY;
      },
      { passive: true },
    );
    const favs = this.app.favoriteProfiles();

    if (favs.length === 0) {
      chips.append(h("span", { class: "fav-empty" }, t("noFavorites")));
    }
    favs.forEach((p, i) => chips.append(this.chip(p, i)));

    const addBtn = h(
      "button",
      { class: "icon-btn", title: t("addConnection"), "aria-label": t("addConnection"), type: "button" },
      icon("plus"),
    );
    addBtn.addEventListener("click", () => this.app.openManager({ create: true }));

    const quick = h("input", {
      class: "quick-input",
      placeholder: t("quickConnectPlaceholder"),
      title: t("quickConnectTitle"),
      spellcheck: "false",
      autocapitalize: "off",
      autocomplete: "off",
      id: "quick-connect",
    });
    quick.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && quick.value.trim()) {
        this.app.openQuick(quick.value.trim());
        quick.value = "";
      } else if (e.key === "Escape") {
        quick.value = "";
        this.app.focusActive();
      }
    });

    const shellBtn = h(
      "button",
      { class: "btn btn-ghost", type: "button", title: `${t("localShell")} (Ctrl+Shift+T)` },
      icon("terminal"),
      h("span", {}, t("localShell")),
      icon("chevron"),
    );
    shellBtn.addEventListener("click", () =>
      showMenuBelow(
        shellBtn,
        this.app.shells.map((s) => ({
          label: s.name,
          icon: icon("terminal"),
          hint: s.isDefault ? "Ctrl+Shift+T" : "",
          action: () => this.app.openShell(s),
        })),
      ),
    );

    const managerBtn = h(
      "button",
      { class: "btn btn-ghost", type: "button", title: `${t("connections")} (Ctrl+Shift+M)` },
      icon("list"),
      h("span", {}, t("connections")),
    );
    managerBtn.addEventListener("click", () => this.app.openManager({}));

    this.root.append(
      h("span", { class: "fav-label", title: t("favorites") }, icon("star")),
      chips,
      addBtn,
      h("div", { class: "spacer" }),
      h("label", { class: "quick" }, icon("bolt"), quick),
      shellBtn,
      managerBtn,
    );
  }

  private chip(p: Profile, index: number): HTMLElement {
    const dot = h("span", { class: "dot" });
    setDotColor(dot, p.color);
    const chip = h(
      "button",
      {
        class: "fav-chip",
        type: "button",
        draggable: "true",
        title: `${describe(p)}${index < 9 ? `  (Ctrl+Shift+${index + 1})` : ""}`,
      },
      dot,
      icon(p.type === "ssh" ? "server" : "terminal"),
      h("span", { class: "fav-name" }, p.name),
    );
    chip.addEventListener("click", () => this.app.openProfile(p));
    chip.addEventListener("auxclick", (e) => {
      if (e.button === 1) this.app.openProfile(p);
    });
    chip.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, [
        { label: t("connect"), icon: icon("bolt"), action: () => this.app.openProfile(p) },
        { label: t("edit"), icon: icon("edit"), action: () => this.app.openManager({ edit: p.id }) },
        { label: t("removeFromFavorites"), icon: icon("star"), action: () => void this.app.setFavorite(p.id, false) },
      ]);
    });

    // Drag & drop reordering.
    chip.addEventListener("dragstart", (e) => {
      this.dragId = p.id;
      chip.classList.add("dragging");
      e.dataTransfer?.setData("text/plain", p.id);
      if (e.dataTransfer) e.dataTransfer.effectAllowed = "move";
    });
    chip.addEventListener("dragend", () => {
      this.dragId = null;
      chip.classList.remove("dragging");
      this.root.querySelectorAll(".drop-before,.drop-after").forEach((el) => el.classList.remove("drop-before", "drop-after"));
    });
    chip.addEventListener("dragover", (e) => {
      if (!this.dragId || this.dragId === p.id) return;
      e.preventDefault();
      const r = chip.getBoundingClientRect();
      const after = e.clientX > r.left + r.width / 2;
      chip.classList.toggle("drop-after", after);
      chip.classList.toggle("drop-before", !after);
    });
    chip.addEventListener("dragleave", () => chip.classList.remove("drop-before", "drop-after"));
    chip.addEventListener("drop", (e) => {
      e.preventDefault();
      const from = this.dragId;
      if (!from || from === p.id) return;
      const r = chip.getBoundingClientRect();
      const after = e.clientX > r.left + r.width / 2;
      const order = this.app.favorites.filter((id) => id !== from);
      const at = order.indexOf(p.id) + (after ? 1 : 0);
      order.splice(at, 0, from);
      void this.app.reorderFavorites(order);
    });
    return chip;
  }
}
