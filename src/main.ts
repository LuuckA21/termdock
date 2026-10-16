import "@xterm/xterm/css/xterm.css";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, errorText } from "./api";
import { alertBox } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe, FavoritesBar, setDotColor } from "./favorites";
import { onLanguageChange, t } from "./i18n";
import { openManager, type ManagerOptions } from "./manager";
import { showMenu } from "./menu";
import { openSettings } from "./settings";
import { TerminalTab } from "./terminal";
import { startUpdateChecks } from "./updates";
import type { AppInfo, DetectedShell, OpenTarget, Profile, Uuid } from "./types";

export class App {
  info!: AppInfo;
  profiles: Profile[] = [];
  favorites: Uuid[] = [];
  shells: DetectedShell[] = [];
  private tabs: TerminalTab[] = [];
  private active: TerminalTab | null = null;
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
    this.active?.focus();
  }

  private openTab(title: string, subtitle: string, target: OpenTarget, color?: string): void {
    const tab = new TerminalTab({
      title,
      subtitle,
      color,
      target,
      vaultAvailable: this.info.vaultAvailable,
      onStateChange: () => this.renderTabs(),
    });
    this.tabs.push(tab);
    this.terminals.append(tab.el);
    this.activate(tab);
    void tab.connect();
  }

  private activate(tab: TerminalTab): void {
    this.active = tab;
    for (const x of this.tabs) x.el.classList.toggle("active", x === tab);
    this.renderTabs();
    requestAnimationFrame(() => tab.focus());
    void getCurrentWindow()
      .setTitle(`${tab.title} — Termdock`)
      .catch(() => {});
  }

  private closeTab(tab: TerminalTab): void {
    const i = this.tabs.indexOf(tab);
    if (i < 0) return;
    this.tabs.splice(i, 1);
    tab.dispose();
    if (this.active === tab) {
      const next = this.tabs[Math.min(i, this.tabs.length - 1)];
      this.active = null;
      if (next) this.activate(next);
      else {
        void getCurrentWindow()
          .setTitle("Termdock")
          .catch(() => {});
      }
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
      setDotColor(dot, tab.opts.color);
      const el = h(
        "div",
        {
          class: `tab ${tab.state}${tab === this.active ? " active" : ""}`,
          role: "tab",
          "aria-selected": String(tab === this.active),
          title: tab.opts.subtitle,
        },
        dot,
        h("span", { class: "tab-title" }, tab.title),
        closeBtn,
      );
      el.addEventListener("click", () => this.activate(tab));
      el.addEventListener("auxclick", (e) => {
        if (e.button === 1) this.closeTab(tab);
      });
      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        showMenu(e.clientX, e.clientY, [
          { label: t("copy"), action: () => void tab.copy() },
          { label: t("paste"), action: () => void tab.paste() },
          { label: t("closeTab"), icon: icon("close"), hint: "Ctrl+Shift+W", action: () => this.closeTab(tab) },
        ]);
      });
      this.tabbar.append(el);
    }
    const add = h("button", { class: "icon-btn tab-add", type: "button", title: `${t("newTab")} (Ctrl+Shift+T)` }, icon("plus"));
    add.addEventListener("click", () => this.openDefaultShell());
    this.tabbar.append(add);
  }

  private onKey(e: KeyboardEvent): void {
    const mod = e.ctrlKey || e.metaKey;
    let handled = true;
    if (mod && e.shiftKey && e.code === "KeyT") this.openDefaultShell();
    else if (mod && e.shiftKey && e.code === "KeyW") {
      if (this.active) this.closeTab(this.active);
    } else if (mod && e.shiftKey && e.code === "KeyK") document.getElementById("quick-connect")?.focus();
    else if (mod && e.shiftKey && e.code === "KeyM") this.openManager({});
    else if (mod && !e.shiftKey && e.code === "Comma") void openSettings();
    else if (mod && e.shiftKey && /^Digit[1-9]$/.test(e.code)) {
      const p = this.favoriteProfiles()[Number(e.code.slice(5)) - 1];
      if (p) this.openProfile(p);
    } else if (e.ctrlKey && e.code === "Tab") this.cycle(e.shiftKey ? -1 : 1);
    else if (e.altKey && !mod && /^Digit[1-9]$/.test(e.code)) {
      const tab = this.tabs[Number(e.code.slice(5)) - 1];
      if (tab) this.activate(tab);
    } else handled = false;
    if (handled) {
      e.preventDefault();
      e.stopPropagation();
    }
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
