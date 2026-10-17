import "@xterm/xterm/css/xterm.css";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, errorText } from "./api";
import { alertBox } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe, FavoritesBar, setDotColor } from "./favorites";
import { onLanguageChange, t } from "./i18n";
import { openManager, type ManagerOptions } from "./manager";
import { showMenu } from "./menu";
import { DEFAULTS, getPrefs, setPrefs } from "./prefs";
import { openSettings } from "./settings";
import { Tab, type Direction, type SplitDir } from "./tabs";
import { TerminalPane } from "./terminal";
import { startUpdateChecks } from "./updates";
import type { AppInfo, DetectedShell, OpenTarget, Profile, Uuid } from "./types";

export class App {
  info!: AppInfo;
  profiles: Profile[] = [];
  favorites: Uuid[] = [];
  shells: DetectedShell[] = [];
  private tabs: Tab[] = [];
  private active: Tab | null = null;
  private readonly favbar: FavoritesBar;
  private readonly tabbar = document.getElementById("tabbar")!;
  private readonly terminals = document.getElementById("terminals")!;

  constructor() {
    this.favbar = new FavoritesBar(document.getElementById("favbar")!, this);
  }

  async init(): Promise<void> {
    [this.info, this.shells] = await Promise.all([api.appInfo(), api.listShells()]);
    await this.reload();
    document.addEventListener("keydown", (e) => this.onKey(e));
    // Language switched in Settings: redraw the chrome; open terminals keep running.
    onLanguageChange(() => {
      this.favbar.render();
      this.renderTabs();
    });
    window.addEventListener("beforeunload", () => this.tabs.forEach((tab) => tab.dispose()));
    // A pane that gets the focus (click, Alt+arrow) becomes the active one.
    this.terminals.addEventListener("focusin", () => this.renderTabs());
    this.renderTabs();
    this.openDefaultShell();
    startUpdateChecks();
  }

  async reload(): Promise<void> {
    const list = await api.listProfiles();
    this.profiles = list.profiles;
    this.favorites = list.favorites;
    this.favbar.render();
  }

  favoriteProfiles(): Profile[] {
    return this.favorites.map((id) => this.profiles.find((p) => p.id === id)).filter((p): p is Profile => !!p);
  }

  async setFavorite(id: Uuid, favorite: boolean): Promise<void> {
    await this.guard(api.setFavorite(id, favorite));
    await this.reload();
  }

  async reorderFavorites(order: Uuid[]): Promise<void> {
    this.favorites = order;
    this.favbar.render();
    await this.guard(api.reorderFavorites(order));
    await this.reload();
  }

  openManager(opts: ManagerOptions): void {
    openManager(this, opts);
  }

  openProfile(p: Profile): void {
    this.openTab(p.name, describe(p), { kind: "profile", id: p.id }, p.color);
  }

  openShell(s: DetectedShell): void {
    this.openTab(s.name, s.shell.program, { kind: "shell", id: s.id });
  }

  openDefaultShell(): void {
    const s = this.shells.find((x) => x.isDefault) ?? this.shells[0];
    if (s) this.openShell(s);
  }

  openQuick(address: string): void {
    this.openTab(address, address, { kind: "quick", address });
  }

  focusActive(): void {
    this.active?.active.focus();
  }

  private newPane(title: string, subtitle: string, target: OpenTarget, color?: string): TerminalPane {
    return new TerminalPane({
      title,
      subtitle,
      color,
      target,
      vaultAvailable: this.info.vaultAvailable,
      onStateChange: () => this.renderTabs(),
      onFocus: (pane) => {
        const tab = this.tabs.find((x) => x.has(pane));
        if (!tab) return;
        tab.setActive(pane, false);
        if (tab === this.active) this.updateWindowTitle();
      },
      isAppShortcut: (e) => this.shortcut(e) !== null,
    });
  }

  private openTab(title: string, subtitle: string, target: OpenTarget, color?: string): void {
    const pane = this.newPane(title, subtitle, target, color);
    const tab = new Tab(pane);
    this.tabs.push(tab);
    this.terminals.append(tab.el);
    this.activate(tab);
    void pane.connect();
  }

  /** Opens the same connection or shell as the active pane next to it. */
  private split(dir: SplitDir): void {
    const tab = this.active;
    if (!tab) return;
    const { title, subtitle, target, color } = tab.active.opts;
    const pane = this.newPane(title, subtitle, target, color);
    tab.split(tab.active, dir, pane);
    tab.setActive(pane, true);
    this.renderTabs();
    void pane.connect();
  }

  private moveFocus(dir: Direction): void {
    const next = this.active?.neighbor(dir);
    if (next) this.active!.setActive(next, true);
  }

  /** Closes the active pane, and the tab with its last pane. */
  private closePane(): void {
    const tab = this.active;
    if (!tab) return;
    if (tab.remove(tab.active)) this.closeTab(tab);
    else {
      tab.active.focus();
      this.renderTabs();
      this.updateWindowTitle();
    }
  }

  /** `setPrefs` keeps the size within its limits. */
  private zoom(delta: number): void {
    setPrefs({ fontSize: delta === 0 ? DEFAULTS.fontSize : getPrefs().fontSize + delta });
  }

  private activate(tab: Tab): void {
    this.active = tab;
    for (const x of this.tabs) x.el.classList.toggle("active", x === tab);
    this.renderTabs();
    requestAnimationFrame(() => tab.active.focus());
    this.updateWindowTitle();
  }

  private updateWindowTitle(): void {
    void getCurrentWindow()
      .setTitle(this.active ? `${this.active.title} — Termdock` : "Termdock")
      .catch(() => {});
  }

  private closeTab(tab: Tab): void {
    const i = this.tabs.indexOf(tab);
    if (i < 0) return;
    this.tabs.splice(i, 1);
    tab.dispose();
    if (this.active === tab) {
      const next = this.tabs[Math.min(i, this.tabs.length - 1)];
      this.active = null;
      if (next) this.activate(next);
      else this.updateWindowTitle();
    }
    this.renderTabs();
  }

  private cycle(delta: number): void {
    if (!this.active || this.tabs.length < 2) return;
    const i = this.tabs.indexOf(this.active);
    this.activate(this.tabs[(i + delta + this.tabs.length) % this.tabs.length]!);
  }

  private renderTabs(): void {
    clear(this.tabbar);
    for (const tab of this.tabs) {
      const closeBtn = h(
        "button",
        { class: "tab-close", type: "button", title: `${t("closeTab")} (Ctrl+Shift+W)`, "aria-label": t("closeTab") },
        icon("close"),
      );
      closeBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.closeTab(tab);
      });
      const dot = h("span", { class: "dot" });
      setDotColor(dot, tab.active.opts.color);
      const count = tab.panes.length;
      const el = h(
        "div",
        {
          class: `tab ${tab.state}${tab === this.active ? " active" : ""}`,
          role: "tab",
          "aria-selected": String(tab === this.active),
          title: tab.active.opts.subtitle,
        },
        dot,
        h("span", { class: "tab-title" }, tab.title),
        count > 1 && h("span", { class: "tab-panes", title: t("panesCount", { count }) }, String(count)),
        closeBtn,
      );
      el.addEventListener("click", () => this.activate(tab));
      el.addEventListener("auxclick", (e) => {
        if (e.button === 1) this.closeTab(tab);
      });
      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        this.activate(tab);
        showMenu(e.clientX, e.clientY, [
          { label: t("splitRight"), icon: icon("splitRight"), hint: "Ctrl+Shift+E", action: () => this.split("right") },
          { label: t("splitDown"), icon: icon("splitDown"), hint: "Ctrl+Shift+O", action: () => this.split("down") },
          { label: t("copy"), action: () => void tab.active.copy() },
          { label: t("paste"), action: () => void tab.active.paste() },
          ...(count > 1 ? [{ label: t("closePane"), hint: "Ctrl+Shift+W", action: () => this.closePane() }] : []),
          { label: t("closeTab"), icon: icon("close"), action: () => this.closeTab(tab) },
        ]);
      });
      this.tabbar.append(el);
    }
    const add = h("button", { class: "icon-btn tab-add", type: "button", title: `${t("newTab")} (Ctrl+Shift+T)` }, icon("plus"));
    add.addEventListener("click", () => this.openDefaultShell());
    this.tabbar.append(add);
  }

  private onKey(e: KeyboardEvent): void {
    const action = this.shortcut(e);
    if (!action) return;
    e.preventDefault();
    e.stopPropagation();
    action();
  }

  /** The app shortcut for this key press, if any. Terminals pass these keys through. */
  private shortcut(e: KeyboardEvent): (() => void) | null {
    if (e.type !== "keydown") return null;
    const mod = e.ctrlKey || e.metaKey;
    const ctrlShift = mod && e.shiftKey && !e.altKey;
    if (ctrlShift) {
      switch (e.code) {
        case "KeyT":
          return () => this.openDefaultShell();
        case "KeyW":
          return () => this.closePane();
        case "KeyK":
          return () => document.getElementById("quick-connect")?.focus();
        case "KeyM":
          return () => this.openManager({});
        case "KeyE":
          return () => this.split("right");
        case "KeyO":
          return () => this.split("down");
      }
      if (/^Digit[1-9]$/.test(e.code)) {
        const p = this.favoriteProfiles()[Number(e.code.slice(5)) - 1];
        return p ? () => this.openProfile(p) : null;
      }
    }
    if (mod && !e.shiftKey && !e.altKey && e.code === "Comma") return () => void openSettings();
    // Font size: Ctrl + "+" / "-" / "0" (by character, so it works on any keyboard layout).
    if (mod && !e.altKey && (e.key === "+" || e.key === "=")) return () => this.zoom(1);
    if (mod && !e.altKey && !e.shiftKey && e.key === "-") return () => this.zoom(-1);
    if (mod && !e.altKey && !e.shiftKey && e.key === "0") return () => this.zoom(0);
    if (e.ctrlKey && !e.altKey && e.code === "Tab") return () => this.cycle(e.shiftKey ? -1 : 1);
    if (e.altKey && !mod && !e.shiftKey) {
      if (/^Digit[1-9]$/.test(e.code)) {
        const tab = this.tabs[Number(e.code.slice(5)) - 1];
        return tab ? () => this.activate(tab) : null;
      }
      // Alt+arrows move between panes, only when there are several:
      // otherwise the shell gets them (word movement in many shells).
      const dir = ({ ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" } as const)[
        e.code as "ArrowLeft"
      ];
      if (dir && this.active && this.active.panes.length > 1) return () => this.moveFocus(dir);
    }
    return null;
  }

  private async guard<T>(p: Promise<T>): Promise<T | undefined> {
    try {
      return await p;
    } catch (e) {
      await alertBox(t("errorTitle"), errorText(e));
      return undefined;
    }
  }
}

const app = new App();
app.init().catch((e) => alertBox(t("errorTitle"), errorText(e)));

// Hide the WebView's own context menu (reload, inspect…) outside text fields.
document.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement;
  if (!el.closest("input, textarea")) e.preventDefault();
});
