import "@xterm/xterm/css/xterm.css";

import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, errorText } from "./api";
import { alertBox } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe, FavoritesBar, setDotColor } from "./favorites";
import { FilesPane } from "./files";
import { onLanguageChange, t } from "./i18n";
import { openManager, type ManagerOptions } from "./manager";
import { showMenu } from "./menu";
import { DEFAULTS, getPrefs, setPrefs } from "./prefs";
import { openSettings } from "./settings";
import { Tab, type Direction, type SplitDir } from "./tabs";
import type { Pane } from "./pane";
import { pickPane, type PaneChoice } from "./picker";
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
      onStateChange: (pane) => {
        // A terminal that ended takes its file browsers' connection with it.
        if (pane.state === "closed") {
          for (const tab of this.tabs) {
            for (const p of tab.panes) if (p instanceof FilesPane && p.opts.source === pane) p.sessionEnded();
          }
        }
        this.renderTabs();
      },
      onFocus: (pane) => this.paneFocused(pane),
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

  /** Opens a file browser on the SSH connection of the active terminal. */
  private openFiles(): void {
    const tab = this.active;
    const source = tab?.active;
    if (!tab || !(source instanceof TerminalPane)) return;
    const session = source.session;
    if (source.opts.target.kind === "shell" || session === null) {
      void alertBox(t("files"), t("filesNeedsSsh"));
      return;
    }
    const { title, subtitle, target, color } = source.opts;
    const files = new FilesPane({
      title,
      subtitle,
      target,
      color,
      session,
      source,
      onStateChange: () => this.renderTabs(),
      onFocus: (pane) => this.paneFocused(pane),
      isAppShortcut: (e) => this.shortcut(e) !== null,
    });
    tab.split(source, "right", files);
    tab.setActive(files, true);
    this.renderTabs();
  }

  /**
   * Splits the active pane and asks what goes in the new half: the same
   * connection (first, so Enter keeps the quick duplicate), a local shell,
   * any saved connection or a quick-connect address.
   */
  private async split(dir: SplitDir): Promise<void> {
    const tab = this.active;
    if (!tab) return;
    const target = tab.active;
    const { title, subtitle, target: current, color } = target.opts;
    const choice = await pickPane({
      title: t(dir === "right" ? "pickTitleRight" : "pickTitleDown"),
      duplicate: { title, subtitle, target: current, color },
      shells: this.shells,
      profiles: this.profiles,
      favorites: this.favorites,
    });
    if (choice && this.tabs.includes(tab) && tab.has(target)) this.splitWith(tab, target, dir, choice);
    else this.focusActive();
  }

  /** Opens `choice` in a new pane next to `target`. */
  private splitWith(tab: Tab, target: Pane, dir: SplitDir, choice: PaneChoice): void {
    const pane = this.newPane(choice.title, choice.subtitle, choice.target, choice.color);
    tab.split(target, dir, pane);
    tab.setActive(pane, true);
    this.renderTabs();
    void pane.connect();
  }

  /** Opens a saved connection next to the active pane (favourites menu). */
  openProfileSplit(p: Profile, dir: SplitDir): void {
    const tab = this.active;
    if (!tab) return this.openProfile(p);
    this.splitWith(tab, tab.active, dir, { title: p.name, subtitle: describe(p), target: { kind: "profile", id: p.id }, color: p.color });
  }

  /**
   * Moves every pane of `from` next to the active pane of `into`, keeping
   * their sessions: two tabs become one split view. `from` closes.
   */
  private mergeTab(from: Tab, into: Tab): void {
    if (from === into) return;
    const panes = from.panes;
    const first = panes[0];
    if (!first) return;
    for (const pane of panes) from.detach(pane);
    into.split(into.active, "right", first);
    let previous = first;
    for (const pane of panes.slice(1)) {
      into.split(previous, "down", pane);
      previous = pane;
    }
    this.tabs.splice(this.tabs.indexOf(from), 1);
    from.el.remove();
    into.setActive(first, false);
    this.activate(into);
  }

  /** A pane that gets the focus (click, Alt+arrow) becomes the active one. */
  private paneFocused(pane: Pane): void {
    const tab = this.tabs.find((x) => x.has(pane));
    if (!tab) return;
    tab.setActive(pane, false);
    this.renderTabs();
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
        { class: "tab-close", type: "button", title: t("closeTab"), "aria-label": t("closeTab") },
        icon("close"),
      );
      closeBtn.addEventListener("click", (e) => {
        e.stopPropagation();
        this.closeTab(tab);
      });
      const dot = h("span", { class: "dot" });
      setDotColor(dot, tab.active.opts.color);
      const count = tab.count;
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
        const active = tab.active;
        showMenu(e.clientX, e.clientY, [
          { label: t("splitRight"), icon: icon("splitRight"), hint: "Ctrl+Shift+E", action: () => void this.split("right") },
          { label: t("splitDown"), icon: icon("splitDown"), hint: "Ctrl+Shift+O", action: () => void this.split("down") },
          // Put this tab next to another one (Windows Terminal style split of two connections).
          ...this.tabs
            .filter((other) => other !== tab)
            .slice(0, 9)
            .map((other) => ({
              label: t("moveNextTo", { name: other.title }),
              icon: icon("splitRight"),
              action: () => this.mergeTab(tab, other),
            })),
          ...(active instanceof TerminalPane
            ? [
                ...(active.opts.target.kind !== "shell" && active.session !== null
                  ? [{ label: t("openFiles"), icon: icon("folder"), hint: "Ctrl+Shift+S", action: () => this.openFiles() }]
                  : []),
                { label: t("copy"), action: () => void active.copy() },
                { label: t("paste"), action: () => void active.paste() },
              ]
            : []),
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
    // Dialogs (settings, host key, password…) keep their keys: nothing may
    // act on the tabs behind them.
    if (document.querySelector("dialog[open]")) return null;
    const mod = e.ctrlKey || e.metaKey;
    const ctrlShift = mod && e.shiftKey && !e.altKey;
    // A held key must not open or close a burst of panes and sessions; the
    // repeats are still swallowed so they do not reach the terminal.
    const once = (action: () => void) => (e.repeat ? () => {} : action);
    if (ctrlShift) {
      switch (e.code) {
        case "KeyT":
          return once(() => this.openDefaultShell());
        case "KeyW":
          return once(() => this.closePane());
        case "KeyK":
          return () => document.getElementById("quick-connect")?.focus();
        case "KeyM":
          return once(() => this.openManager({}));
        case "KeyE":
          return once(() => void this.split("right"));
        case "KeyO":
          return once(() => void this.split("down"));
        case "KeyS":
          return once(() => this.openFiles());
      }
      if (/^Digit[1-9]$/.test(e.code)) {
        const p = this.favoriteProfiles()[Number(e.code.slice(5)) - 1];
        return p ? once(() => this.openProfile(p)) : null;
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
      // Alt+arrows move between panes, only inside a terminal and when there
      // are several: otherwise the shell or text field gets them (word movement).
      const dir = ({ ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" } as const)[
        e.code as "ArrowLeft"
      ];
      // Not from text fields (the file browser's path): they keep word movement.
      const inTerminal =
        e.target instanceof Element && this.terminals.contains(e.target) && !e.target.closest("input");
      if (dir && inTerminal && this.active && this.active.count > 1) return () => this.moveFocus(dir);
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
