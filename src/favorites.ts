import { h, icon, clear } from "./dom";
import { t } from "./i18n";
import { digitHint, keyHint, withKey } from "./keys";
import { showMenu, showMenuBelow } from "./menu";
import { openSettings } from "./settings";
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
  /** Set right after a drag, so its click does not open the connection. */
  private suppressClick = false;

  constructor(
    private readonly root: HTMLElement,
    private readonly app: App,
  ) {}

  render(): void {
    clear(this.root);
    this.root.setAttribute("aria-label", t("favorites"));
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
      { class: "btn btn-ghost", type: "button", title: withKey(t("localShell"), "newShell") },
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
          hint: s.isDefault ? keyHint("newShell") : "",
          action: () => this.app.openShell(s),
        })),
      ),
    );

    const layoutsBtn = h(
      "button",
      { class: "btn btn-ghost", type: "button", title: t("layouts") },
      icon("splitRight"),
      h("span", {}, t("layouts")),
      icon("chevron"),
    );
    layoutsBtn.addEventListener("click", () => this.app.layoutsMenu(layoutsBtn));

    const snippetsBtn = h(
      "button",
      { class: "btn btn-ghost", type: "button", title: withKey(t("snippets"), "snippets") },
      icon("snippet"),
      h("span", {}, t("snippets")),
      icon("chevron"),
    );
    snippetsBtn.addEventListener("click", () => this.app.snippetsMenu(snippetsBtn));

    const managerBtn = h(
      "button",
      { class: "btn btn-ghost", type: "button", title: withKey(t("connections"), "connections") },
      icon("list"),
      h("span", {}, t("connections")),
    );
    managerBtn.addEventListener("click", () => this.app.openManager({}));

    const settingsBtn = h(
      "button",
      { class: "icon-btn", type: "button", title: withKey(t("settings"), "settings"), "aria-label": t("settings") },
      icon("settings"),
    );
    settingsBtn.addEventListener("click", () => void openSettings());

    this.root.append(
      h("span", { class: "fav-label", title: t("favorites") }, icon("star")),
      chips,
      addBtn,
      h("div", { class: "spacer" }),
      h("label", { class: "quick" }, icon("bolt"), quick),
      shellBtn,
      layoutsBtn,
      snippetsBtn,
      managerBtn,
      settingsBtn,
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
        "data-id": p.id,
        title: index < 9 && digitHint("openFavorite", index + 1) ? `${describe(p)}  (${digitHint("openFavorite", index + 1)})` : describe(p),
      },
      dot,
      icon(p.type === "ssh" ? "server" : "terminal"),
      h("span", { class: "fav-name" }, p.name),
    );
    chip.addEventListener("click", () => {
      if (!this.suppressClick) this.app.openProfile(p);
    });
    chip.addEventListener("auxclick", (e) => {
      if (e.button === 1) this.app.openProfile(p);
    });
    chip.addEventListener("contextmenu", (e) => {
      e.preventDefault();
      showMenu(e.clientX, e.clientY, [
        { label: t("connect"), icon: icon("bolt"), action: () => this.app.openProfile(p) },
        { label: t("openSplitRight"), icon: icon("splitRight"), action: () => this.app.openProfileSplit(p, "right") },
        { label: t("openSplitDown"), icon: icon("splitDown"), action: () => this.app.openProfileSplit(p, "down") },
        { label: t("edit"), icon: icon("edit"), action: () => this.app.openManager({ edit: p.id }) },
        { label: t("removeFromFavorites"), icon: icon("star"), action: () => void this.app.setFavorite(p.id, false) },
      ]);
    });

    // Reordering by dragging with the pointer (not HTML5 drag & drop: the
    // window takes file drops from the system for the file browser).
    chip.addEventListener("pointerdown", (start) => {
      if (start.button !== 0) return;
      // Captured at once, so the release is always seen here.
      chip.setPointerCapture(start.pointerId);
      let dragging = false;
      let target: { id: string; after: boolean } | null = null;
      const clear = () =>
        this.root.querySelectorAll(".drop-before,.drop-after").forEach((el) => el.classList.remove("drop-before", "drop-after"));
      const move = (e: PointerEvent) => {
        if (!(e.buttons & 1)) return end();
        if (!dragging && Math.hypot(e.clientX - start.clientX, e.clientY - start.clientY) < 6) return;
        if (!dragging) {
          dragging = true;
          chip.classList.add("dragging");
        }
        clear();
        target = null;
        const over = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>(".fav-chip");
        const id = over?.dataset.id;
        if (!over || !id || id === p.id) return;
        const r = over.getBoundingClientRect();
        const after = e.clientX > r.left + r.width / 2;
        over.classList.add(after ? "drop-after" : "drop-before");
        target = { id, after };
      };
      const self = this;
      function end(): void {
        chip.removeEventListener("pointermove", move);
        chip.removeEventListener("pointerup", end);
        chip.removeEventListener("pointercancel", end);
        chip.classList.remove("dragging");
        clear();
        if (!dragging) return;
        // The click that follows a drag must not open the connection.
        self.suppressClick = true;
        setTimeout(() => (self.suppressClick = false), 0);
        if (!target) return;
        const order = self.app.favorites.filter((id) => id !== p.id);
        order.splice(order.indexOf(target.id) + (target.after ? 1 : 0), 0, p.id);
        void self.app.reorderFavorites(order);
      }
      chip.addEventListener("pointermove", move);
      chip.addEventListener("pointerup", end);
      chip.addEventListener("pointercancel", end);
    });
    return chip;
  }
}
