import "@xterm/xterm/css/xterm.css";

import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";

import { api, errorText } from "./api";
import { alertBox } from "./dialogs";
import { clear, h, icon } from "./dom";
import { describe, FavoritesBar, setDotColor } from "./favorites";
import { carriesFiles, FilesPane } from "./files";
import { onLanguageChange, t } from "./i18n";
import { openManager, type ManagerOptions } from "./manager";
import { firstLeaf, getLayouts, MAX_PANES, paneCount, saveLayout as saveLayoutAs, startupLayout, type LayoutLeaf, type LayoutNode } from "./layouts";
import { askLayoutName, manageLayouts } from "./layoutsui";
import { actionFor, digitFor, digitHint, keyHint, onKeysChange, withKey } from "./keys";
import { showMenu, showMenuBelow } from "./menu";
import { DEFAULTS, getPrefs, setPrefs } from "./prefs";
import { openSettings } from "./settings";
import { fitsGroup, getSnippets, type Snippet } from "./snippets";
import { Tab, type Direction, type Snapshot, type SplitDir } from "./tabs";
import type { Pane } from "./pane";
import { pickPane, type PaneChoice } from "./picker";
import { TerminalPane } from "./terminal";
import { startUpdateChecks } from "./updates";
import type { AppInfo, DetectedShell, OpenTarget, PickedFile, Profile, Uuid } from "./types";

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
    this.listenForDrops();
    // Language switched in Settings: redraw the chrome and relabel the open
    // panes; their sessions keep running.
    onLanguageChange(() => {
      this.favbar.render();
      this.renderTabs();
      for (const tab of this.tabs) for (const pane of tab.panes) pane.relabel();
    });
    // Shortcuts changed in Settings: redraw the hints.
    onKeysChange(() => {
      this.favbar.render();
      this.renderTabs();
    });
    window.addEventListener("beforeunload", () => this.tabs.forEach((tab) => tab.dispose()));
    this.renderTabs();
    const startup = startupLayout();
    if (startup) this.openLayout(startup.id);
    else this.openDefaultShell();
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

  /** Quick connect is always SSH; a saved connection may be a local shell. */
  private isSsh(pane: TerminalPane): boolean {
    const target = pane.opts.target;
    if (target.kind === "quick") return true;
    return target.kind === "profile" && this.profiles.find((p) => p.id === target.id)?.type === "ssh";
  }

  /** Opens a file browser on the SSH connection of the active terminal. */
  private openFiles(): void {
    const tab = this.active;
    const source = tab?.active;
    if (!tab || !(source instanceof TerminalPane)) return;
    const session = source.session;
    if (!this.isSsh(source) || session === null) {
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
    // The menu may be older than the tabs: both must still be open.
    if (from === into || !this.tabs.includes(from) || !this.tabs.includes(into)) return;
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

  /** Saves the tab's split layout (terminals only) under a name. */
  async saveLayout(tab: Tab | null = this.active): Promise<void> {
    if (!tab) return;
    const snapshot = tab.snapshot((pane) =>
      pane instanceof TerminalPane ? { target: pane.opts.target, title: pane.opts.title } : null,
    );
    if (!snapshot) return;
    const toNode = (s: Snapshot<{ target: OpenTarget; title: string }>): LayoutNode =>
      s.kind === "pane"
        ? { kind: "pane", target: s.value.target, title: s.value.title }
        : { kind: "split", dir: s.dir, ratio: s.ratio, a: toNode(s.a), b: toNode(s.b) };
    const root = toNode(snapshot);
    if (paneCount(root) > MAX_PANES) {
      await alertBox(t("layouts"), t("layoutTooMany", { count: MAX_PANES }));
      return;
    }
    const name = await askLayoutName(t("layoutSave"), tab.title, null);
    if (name && !saveLayoutAs(name, root)) await alertBox(t("layouts"), t("layoutInvalid"));
    this.focusActive();
  }

  /**
   * Opens a saved layout in a new tab. Connections or shells that no longer
   * exist open the default local shell instead, and the user is told.
   */
  openLayout(id: string): void {
    const layout = getLayouts().find((l) => l.id === id);
    if (!layout) return;
    const missing: string[] = [];
    const fallback = this.shells.find((x) => x.isDefault) ?? this.shells[0];
    const resolve = (leaf: LayoutLeaf): PaneChoice | null => {
      const { target } = leaf;
      if (target.kind === "profile") {
        const p = this.profiles.find((x) => x.id === target.id);
        if (p) return { title: p.name, subtitle: describe(p), target, color: p.color };
      } else if (target.kind === "shell") {
        const s = this.shells.find((x) => x.id === target.id);
        if (s) return { title: s.name, subtitle: s.shell.program, target };
      } else {
        return { title: target.address, subtitle: target.address, target };
      }
      missing.push(leaf.title);
      return fallback ? { title: fallback.name, subtitle: fallback.shell.program, target: { kind: "shell", id: fallback.id } } : null;
    };
    const make = (leaf: LayoutLeaf): TerminalPane | null => {
      const c = resolve(leaf);
      return c ? this.newPane(c.title, c.subtitle, c.target, c.color) : null;
    };
    const first = make(firstLeaf(layout.root));
    if (!first) {
      void alertBox(layout.name, t("layoutMissing", { names: missing.join(", ") }));
      return;
    }
    const tab = new Tab(first);
    const panes = [first];
    // `pane` fills the area of `node`: split it as the saved tree says.
    const place = (node: LayoutNode, pane: TerminalPane): void => {
      if (node.kind === "pane") return;
      const created = make(firstLeaf(node.b));
      // Only without any local shell: keep what can be opened.
      if (!created) return place(node.a, pane);
      panes.push(created);
      tab.split(pane, node.dir, created, node.ratio);
      place(node.a, pane);
      place(node.b, created);
    };
    place(layout.root, first);
    this.tabs.push(tab);
    this.terminals.append(tab.el);
    tab.setActive(first, false);
    this.activate(tab);
    for (const pane of panes) void pane.connect();
    if (missing.length) void alertBox(layout.name, t("layoutMissing", { names: missing.join(", ") }));
  }

  /** The layouts menu (toolbar button). */
  layoutsMenu(anchor: HTMLElement): void {
    const startup = startupLayout()?.id;
    showMenuBelow(anchor, [
      ...getLayouts().map((l) => ({
        label: l.name,
        icon: icon("splitRight"),
        hint: l.id === startup ? t("layoutAtStartup") : t("layoutPanes", { count: paneCount(l.root) }),
        action: () => this.openLayout(l.id),
      })),
      { label: t("layoutSave"), icon: icon("plus"), hint: keyHint("saveLayout"), action: () => void this.saveLayout() },
      { label: t("layoutManage"), icon: icon("edit"), action: () => void manageLayouts((id) => this.openLayout(id)) },
    ]);
  }

  /** The group of the saved connection in `pane`; null for shells and quick connect. */
  private groupOf(pane: TerminalPane): string | null {
    const target = pane.opts.target;
    if (target.kind !== "profile") return null;
    return this.profiles.find((p) => p.id === target.id)?.group?.trim() || null;
  }

  /** The active terminal pane, if the active pane is one. */
  private activeTerminal(): TerminalPane | null {
    const pane = this.active?.active;
    return pane instanceof TerminalPane ? pane : null;
  }

  private sendSnippet(pane: TerminalPane, s: Snippet): void {
    void pane.sendSnippet(s.text, s.run);
  }

  /**
   * The snippets menu (toolbar button, shortcut, tab menu): the snippets for
   * the active terminal's group, numbered like their 1…9 shortcuts.
   */
  snippetsMenu(anchor?: HTMLElement): void {
    const pane = this.activeTerminal();
    const group = pane ? this.groupOf(pane) : null;
    const items = getSnippets()
      .map((s, i) => ({ s, n: i + 1 }))
      .filter(({ s }) => pane && fitsGroup(s, group))
      .map(({ s, n }) => ({
        label: s.name,
        icon: icon(s.run ? "terminal" : "snippet"),
        hint: n <= 9 ? digitHint("sendSnippet", n) : "",
        action: () => pane && this.sendSnippet(pane, s),
      }));
    const manage = { label: t("snippetsManage"), icon: icon("edit"), action: () => void openSettings("snippets") };
    const none = {
      label: pane ? t("snippetsNone") : t("snippetsNoTerminal"),
      action: () => void openSettings("snippets"),
    };
    const all = [...(items.length ? items : [none]), manage];
    if (anchor) {
      showMenuBelow(anchor, all);
    } else {
      const r = (pane?.el ?? this.terminals).getBoundingClientRect();
      showMenu(r.left + 24, r.top + 24, all);
    }
  }

  /** The file browser at a point of the window (CSS pixels), if any. */
  private filesAt(x: number, y: number): FilesPane | null {
    const el = document.elementFromPoint(x, y)?.closest(".files-pane");
    for (const tab of this.tabs) {
      for (const pane of tab.panes) if (pane instanceof FilesPane && pane.el === el) return pane;
    }
    return null;
  }

  /** Files dragged from the system: highlight and upload where they land. */
  private listenForDrops(): void {
    let hovered: FilesPane | null = null;
    void listen<{ x: number; y: number; active: boolean }>("termdock://drag", ({ payload }) => {
      const pane = payload.active ? this.filesAt(payload.x, payload.y) : null;
      if (pane !== hovered) {
        hovered?.setDropHover(false);
        pane?.setDropHover(true);
        hovered = pane;
      }
    });
    void listen<{ x: number; y: number; files: PickedFile[] }>("termdock://drop", ({ payload }) => {
      hovered?.setDropHover(false);
      hovered = null;
      const pane = this.filesAt(payload.x, payload.y);
      // Not on a file browser (or it is busy): nothing is uploaded.
      if (!pane?.dropFiles(payload.files) && payload.files.length) {
        void api.sftpForgetPicks(payload.files.map((f) => f.token)).catch(() => {});
      }
    });
  }

  /** A pane that gets the focus (click, keyboard) becomes the active one. */
  private paneFocused(pane: Pane): void {
    const tab = this.tabs.find((x) => x.has(pane));
    // Every click in a pane lands here: redraw only when the active pane changes.
    if (!tab || tab.active === pane) return;
    tab.setActive(pane, false);
    this.renderTabs();
    this.updateWindowTitle();
  }

  private moveFocus(dir: Direction): void {
    const next = this.active?.neighbor(dir);
    if (next) this.active!.setActive(next, true);
  }

  /** Closes the active pane, and the tab with its last pane. */
  private closePane(): void {
    const tab = this.active;
    if (!tab) return;
    this.endFileBrowsersOf(tab.active);
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

  private activate(tab: Tab, focus = true): void {
    this.active = tab;
    for (const x of this.tabs) x.el.classList.toggle("active", x === tab);
    this.renderTabs();
    if (focus) requestAnimationFrame(() => tab.active.focus());
    this.updateWindowTitle();
  }

  private updateWindowTitle(): void {
    void getCurrentWindow()
      .setTitle(this.active ? `${this.active.title} — Termdock` : "Termdock")
      .catch(() => {});
  }

  /** A terminal is closing: the file browsers on its connection end with it. */
  private endFileBrowsersOf(pane: Pane): void {
    if (!(pane instanceof TerminalPane)) return;
    for (const tab of this.tabs) {
      for (const p of tab.panes) if (p instanceof FilesPane && p.opts.source === pane) p.sessionEnded();
    }
  }

  private closeTab(tab: Tab): void {
    const i = this.tabs.indexOf(tab);
    if (i < 0) return;
    for (const pane of tab.panes) this.endFileBrowsersOf(pane);
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
      const waiting = tab.waiting;
      const el = h(
        "div",
        {
          class: `tab ${tab.state}${tab === this.active ? " active" : ""}${waiting ? " waiting" : ""}`,
          role: "tab",
          "aria-selected": String(tab === this.active),
          title: tab.active.opts.subtitle,
        },
        dot,
        h("span", { class: "tab-title" }, tab.title),
        waiting && h("span", { class: "tab-waiting", title: t("tabWaiting"), "aria-label": t("tabWaiting") }, icon("lock")),
        count > 1 && h("span", { class: "tab-panes", title: t("panesCount", { count }) }, String(count)),
        closeBtn,
      );
      el.addEventListener("click", () => this.activate(tab));
      el.addEventListener("auxclick", (e) => {
        if (e.button === 1) this.closeTab(tab);
      });
      el.addEventListener("contextmenu", (e) => {
        e.preventDefault();
        // The menu keeps the keyboard focus: the terminal gets it back when it closes.
        this.activate(tab, false);
        const active = tab.active;
        showMenu(e.clientX, e.clientY, [
          { label: t("splitRight"), icon: icon("splitRight"), hint: keyHint("splitRight"), action: () => void this.split("right") },
          { label: t("splitDown"), icon: icon("splitDown"), hint: keyHint("splitDown"), action: () => void this.split("down") },
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
                ...(this.isSsh(active) && active.session !== null
                  ? [{ label: t("openFiles"), icon: icon("folder"), hint: keyHint("files"), action: () => this.openFiles() }]
                  : []),
                { label: t("find"), hint: keyHint("find"), action: () => active.find() },
                { label: t("copy"), hint: keyHint("copy"), action: () => void active.copy() },
                { label: t("paste"), hint: keyHint("paste"), action: () => void active.paste() },
                { label: t("snippets"), icon: icon("snippet"), hint: keyHint("snippets"), action: () => this.snippetsMenu() },
              ]
            : []),
          { label: t("layoutSave"), icon: icon("plus"), hint: keyHint("saveLayout"), action: () => void this.saveLayout(tab) },
          ...(count > 1 ? [{ label: t("closePane"), hint: keyHint("closePane"), action: () => this.closePane() }] : []),
          { label: t("closeTab"), icon: icon("close"), action: () => this.closeTab(tab) },
        ]);
      });
      this.tabbar.append(el);
    }
    const add = h("button", { class: "icon-btn tab-add", type: "button", title: withKey(t("newTab"), "newShell") }, icon("plus"));
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
    // A held key must not open or close a burst of panes and sessions; the
    // repeats are still swallowed so they do not reach the terminal.
    const once = (action: () => void) => (e.repeat ? () => {} : action);
    const digit = digitFor(e);
    if (digit?.action === "openFavorite") {
      const p = this.favoriteProfiles()[digit.n - 1];
      return p ? once(() => this.openProfile(p)) : null;
    }
    if (digit?.action === "sendSnippet") {
      // Numbered over the whole list, like in Settings; only if it fits this terminal.
      const pane = this.activeTerminal();
      const s = getSnippets()[digit.n - 1];
      return pane && s && fitsGroup(s, this.groupOf(pane)) ? once(() => this.sendSnippet(pane, s)) : null;
    }
    if (digit?.action === "goToTab") {
      const tab = this.tabs[digit.n - 1];
      return tab ? () => this.activate(tab) : null;
    }
    const action = actionFor(e);
    switch (action) {
      case "newShell":
        return once(() => this.openDefaultShell());
      case "closePane":
        return once(() => this.closePane());
      case "quickConnect":
        return () => document.getElementById("quick-connect")?.focus();
      case "connections":
        return once(() => this.openManager({}));
      case "splitRight":
        return once(() => void this.split("right"));
      case "splitDown":
        return once(() => void this.split("down"));
      case "files":
        return once(() => this.openFiles());
      case "saveLayout":
        return once(() => void this.saveLayout());
      case "find": {
        const pane = this.active?.active;
        return pane instanceof TerminalPane ? once(() => pane.find()) : null;
      }
      case "settings":
        return () => void openSettings();
      case "snippets":
        return once(() => this.snippetsMenu());
      case "zoomIn":
        return () => this.zoom(1);
      case "zoomOut":
        return () => this.zoom(-1);
      case "zoomReset":
        return () => this.zoom(0);
      case "nextTab":
        return () => this.cycle(1);
      case "prevTab":
        return () => this.cycle(-1);
      case "movePaneLeft":
      case "movePaneRight":
      case "movePaneUp":
      case "movePaneDown": {
        // Only inside a terminal and with several panes: otherwise the shell
        // or the text field (the file browser's path) keeps the key.
        const inTerminal =
          e.target instanceof Element && this.terminals.contains(e.target) && !e.target.closest("input");
        if (!inTerminal || !this.active || this.active.count < 2) return null;
        const dir = ({ movePaneLeft: "left", movePaneRight: "right", movePaneUp: "up", movePaneDown: "down" } as const)[action];
        return () => this.moveFocus(dir);
      }
      // Copy and paste are handled by the terminal itself.
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

// Files dragged from the system are taken by the backend (see
// listenForDrops). The web view itself never opens a dropped file or link:
// it only accepts the drag over a file browser, so the drop happens there.
for (const type of ["dragover", "drop"] as const) {
  document.addEventListener(type, (e) => {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    const overFiles = e.target instanceof Element && e.target.closest(".files-pane");
    if (e.dataTransfer) e.dataTransfer.dropEffect = overFiles ? "copy" : "none";
  });
}

// Hide the WebView's own context menu (reload, inspect…) outside text fields.
document.addEventListener("contextmenu", (e) => {
  const el = e.target as HTMLElement;
  if (!el.closest("input, textarea")) e.preventDefault();
});
